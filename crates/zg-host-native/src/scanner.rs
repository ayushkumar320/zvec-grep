use std::{
    collections::HashSet,
    fs::{self, File, Metadata},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Instant, UNIX_EPOCH},
};

use async_trait::async_trait;
use same_file::is_same_file;
use tokio::sync::Semaphore;

use crate::{
    HostError,
    api::{
        DiscoveredFile, FileKind, KnownSourceFile, ReadBatchRequest, RootSpec, ScanDiagnostics,
        ScanRequest, ScanSnapshot, SkippedFile, SkippedFileReason, SourceFile, TaskControl,
        WorkspaceScannerPort,
    },
    file_type::{detect_file_type, max_file_size},
    pattern::normalize_relative_path,
    policy::{FileTypeResolver, IgnoreRule, RootPolicy},
};

const BINARY_SNIFF_BYTES: usize = 8_192;
const BINARY_CONTROL_CHAR_PERCENT: usize = 30;
const MAX_SKIPPED_FILE_SAMPLES: usize = 20;

#[derive(Clone, Debug)]
pub struct NativeScanner {
    resolver: FileTypeResolver,
    scan_slots: Arc<Semaphore>,
}

impl NativeScanner {
    #[must_use]
    pub fn new() -> Self {
        Self {
            resolver: FileTypeResolver::new(),
            scan_slots: Arc::new(Semaphore::new(1)),
        }
    }

    #[must_use]
    pub fn with_max_concurrent_scans(mut self, maximum: usize) -> Self {
        self.scan_slots = Arc::new(Semaphore::new(maximum.max(1)));
        self
    }
}

impl Default for NativeScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WorkspaceScannerPort for NativeScanner {
    async fn discover(
        &self,
        request: &ScanRequest,
        control: &TaskControl,
    ) -> Result<ScanSnapshot, HostError> {
        let _permit = acquire_slot(&self.scan_slots, control).await?;
        let request = request.clone();
        let resolver = self.resolver.clone();
        run_blocking(control, move |blocking_control| {
            discover_sync(&request, &resolver, &blocking_control)
        })
        .await
    }

    async fn read_batch(
        &self,
        request: &ReadBatchRequest,
        control: &TaskControl,
    ) -> Result<Vec<SourceFile>, HostError> {
        let _permit = acquire_slot(&self.scan_slots, control).await?;
        let request = request.clone();
        run_blocking(control, move |blocking_control| {
            read_batch_sync(&request, &blocking_control)
        })
        .await
    }
}

#[derive(Clone, Debug)]
struct BlockingControl {
    cancellation: tokio_util::sync::CancellationToken,
    deadline: Option<Instant>,
}

impl BlockingControl {
    fn check(&self) -> Result<(), HostError> {
        if self.cancellation.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(HostError::DeadlineExceeded);
        }
        Ok(())
    }
}

async fn acquire_slot<'a>(
    slots: &'a Semaphore,
    control: &TaskControl,
) -> Result<tokio::sync::SemaphorePermit<'a>, HostError> {
    control_check(control)?;
    tokio::select! {
        () = control.cancellation.cancelled() => Err(HostError::Cancelled),
        () = deadline_wait(control.deadline) => Err(HostError::DeadlineExceeded),
        permit = slots.acquire() => permit.map_err(|_| HostError::Closed),
    }
}

async fn run_blocking<T, F>(control: &TaskControl, function: F) -> Result<T, HostError>
where
    T: Send + 'static,
    F: FnOnce(BlockingControl) -> Result<T, HostError> + Send + 'static,
{
    control_check(control)?;
    let blocking_control = BlockingControl {
        cancellation: control.cancellation.clone(),
        deadline: control.deadline,
    };
    let task = tokio::task::spawn_blocking(move || function(blocking_control));
    tokio::select! {
        () = control.cancellation.cancelled() => Err(HostError::Cancelled),
        () = deadline_wait(control.deadline) => Err(HostError::DeadlineExceeded),
        result = task => result
            .map_err(|error| HostError::Internal { message: format!("native scanner worker failed: {error}") })?,
    }
}

async fn deadline_wait(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
    } else {
        std::future::pending::<()>().await;
    }
}

fn control_check(control: &TaskControl) -> Result<(), HostError> {
    if control.cancellation.is_cancelled() {
        return Err(HostError::Cancelled);
    }
    if control
        .deadline
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return Err(HostError::DeadlineExceeded);
    }
    Ok(())
}

#[derive(Debug)]
struct ScanDomain {
    policy: RootPolicy,
    canonical_path: PathBuf,
    metadata: Metadata,
}

fn discover_sync(
    request: &ScanRequest,
    resolver: &FileTypeResolver,
    control: &BlockingControl,
) -> Result<ScanSnapshot, HostError> {
    control.check()?;
    let domains = validate_domains(&request.roots, resolver)?;
    let known_files = normalize_known_files(&request.known_files);
    let mut files = Vec::new();
    let mut diagnostics = ScanDiagnostics::default();

    for domain in domains {
        control.check()?;
        if domain.metadata.is_file() {
            scan_root_file(
                &domain.policy,
                &known_files,
                &mut files,
                &mut diagnostics,
                control,
            )?;
        } else if domain.metadata.is_dir() {
            scan_root_directory(&domain, &known_files, &mut files, &mut diagnostics, control)?;
        }
    }
    files.sort_by(|left, right| {
        left.root
            .cmp(&right.root)
            .then(left.relative_path.cmp(&right.relative_path))
    });
    files.dedup_by(|left, right| {
        left.root == right.root && left.relative_path == right.relative_path
    });
    Ok(ScanSnapshot { files, diagnostics })
}

fn validate_domains(
    roots: &[RootSpec],
    resolver: &FileTypeResolver,
) -> Result<Vec<ScanDomain>, HostError> {
    let mut domains = Vec::with_capacity(roots.len());
    for root in roots {
        let policy = RootPolicy::new(root.clone(), resolver)?;
        let metadata = fs::metadata(policy.root_path()).map_err(|error| {
            HostError::invalid_input(format!(
                "workspace root {} could not be inspected: {error}",
                policy.root_path().display()
            ))
        })?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(HostError::invalid_input(format!(
                "workspace root {} must be a file or directory",
                policy.root_path().display()
            )));
        }
        let canonical_path = fs::canonicalize(policy.root_path()).map_err(|error| {
            HostError::invalid_input(format!(
                "workspace root {} could not be resolved: {error}",
                policy.root_path().display()
            ))
        })?;
        domains.push(ScanDomain {
            policy,
            canonical_path,
            metadata,
        });
    }
    for left_index in 0..domains.len() {
        for right_index in (left_index + 1)..domains.len() {
            if domains_overlap(&domains[left_index], &domains[right_index])? {
                return Err(HostError::invalid_input(format!(
                    "workspace roots overlap: left={} right={}",
                    domains[left_index].policy.root_path().display(),
                    domains[right_index].policy.root_path().display()
                )));
            }
        }
    }
    Ok(domains)
}

fn domains_overlap(left: &ScanDomain, right: &ScanDomain) -> Result<bool, HostError> {
    if is_same_file(left.policy.root_path(), right.policy.root_path()).map_err(|error| {
        HostError::backend(
            "native-scanner",
            format!("root identity check failed: {error}"),
        )
    })? {
        return Ok(true);
    }
    let left_file = left.metadata.is_file();
    let right_file = right.metadata.is_file();
    if left_file && right_file {
        return Ok(false);
    }
    if !left_file && !right_file {
        return Ok(
            directory_covers_directory(left, right) || directory_covers_directory(right, left)
        );
    }
    let (directory, file) = if left_file {
        (right, left)
    } else {
        (left, right)
    };
    Ok(directory_covers_file(directory, &file.canonical_path))
}

fn directory_covers_directory(directory: &ScanDomain, child: &ScanDomain) -> bool {
    directory.policy.root().recursive && child.canonical_path.starts_with(&directory.canonical_path)
}

fn directory_covers_file(directory: &ScanDomain, file: &Path) -> bool {
    file.starts_with(&directory.canonical_path)
        && (directory.policy.root().recursive || file.parent() == Some(&directory.canonical_path))
}

fn normalize_known_files(known_files: &[KnownSourceFile]) -> HashSet<KnownFileKey> {
    known_files
        .iter()
        .map(|known| KnownFileKey {
            root: std::path::absolute(&known.root).unwrap_or_else(|_| known.root.clone()),
            relative_path: known.relative_path.clone(),
            source_fingerprint: known.source_fingerprint.clone(),
        })
        .collect()
}

#[derive(Debug, Eq, Hash, PartialEq)]
struct KnownFileKey {
    root: PathBuf,
    relative_path: PathBuf,
    source_fingerprint: String,
}

fn scan_root_file(
    policy: &RootPolicy,
    known_files: &HashSet<KnownFileKey>,
    files: &mut Vec<DiscoveredFile>,
    diagnostics: &mut ScanDiagnostics,
    control: &BlockingControl,
) -> Result<(), HostError> {
    let Some(name) = policy.root_path().file_name() else {
        return Ok(());
    };
    let relative_path = PathBuf::from(name);
    let relative_display = normalize_relative_path(&relative_path);
    if !policy.matches_file_selection(&relative_display) {
        return Ok(());
    }
    if let Some(file) = read_file_info(
        policy,
        policy.root_path(),
        &relative_path,
        known_files,
        diagnostics,
        control,
    )? {
        files.push(file);
    }
    Ok(())
}

fn scan_root_directory(
    domain: &ScanDomain,
    known_files: &HashSet<KnownFileKey>,
    files: &mut Vec<DiscoveredFile>,
    diagnostics: &mut ScanDiagnostics,
    control: &BlockingControl,
) -> Result<(), HostError> {
    let root_name = domain
        .policy
        .root_path()
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    if RootPolicy::is_hard_skipped_name(&root_name) {
        return Ok(());
    }
    let mut visited = HashSet::from([domain.canonical_path.clone()]);
    walk(
        &domain.policy,
        domain.policy.root_path(),
        0,
        &domain.policy.initial_ignore_rules(),
        &mut visited,
        known_files,
        files,
        diagnostics,
        control,
    )
}

#[allow(clippy::too_many_arguments)]
fn walk(
    policy: &RootPolicy,
    current_path: &Path,
    depth: usize,
    parent_ignore_rules: &[IgnoreRule],
    visited: &mut HashSet<PathBuf>,
    known_files: &HashSet<KnownFileKey>,
    files: &mut Vec<DiscoveredFile>,
    diagnostics: &mut ScanDiagnostics,
    control: &BlockingControl,
) -> Result<(), HostError> {
    control.check()?;
    let ignore_rules = policy.rules_with_gitignore(parent_ignore_rules, current_path);
    let Ok(read_directory) = fs::read_dir(current_path) else {
        return Ok(());
    };
    let mut entries: Vec<_> = read_directory.filter_map(Result::ok).collect();
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        control.check()?;
        let absolute_path = entry.path();
        let relative_path = absolute_path
            .strip_prefix(policy.root_path())
            .map_err(|error| HostError::Internal {
                message: format!("scanner produced an out-of-root path: {error}"),
            })?;
        let relative_display = normalize_relative_path(relative_path);
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let (is_directory, is_file) = if file_type.is_symlink() {
            if !policy.root().discovery.follow {
                continue;
            }
            fs::metadata(&absolute_path).map_or((false, false), |metadata| {
                (metadata.is_dir(), metadata.is_file())
            })
        } else {
            (file_type.is_dir(), file_type.is_file())
        };

        if is_directory {
            if !policy.root().recursive
                || policy
                    .root()
                    .discovery
                    .max_depth
                    .is_some_and(|maximum| depth + 1 >= maximum)
                || !policy.path_can_be_scanned(&relative_display, &name, true, &ignore_rules)
            {
                continue;
            }
            if is_nested_git_repository_directory(&absolute_path)
                && !policy.nested_git_repository_explicitly_included(&relative_display)
            {
                continue;
            }
            let Ok(canonical) = fs::canonicalize(&absolute_path) else {
                continue;
            };
            if !visited.insert(canonical) {
                continue;
            }
            walk(
                policy,
                &absolute_path,
                depth + 1,
                &ignore_rules,
                visited,
                known_files,
                files,
                diagnostics,
                control,
            )?;
            continue;
        }
        if !is_file
            || policy
                .root()
                .discovery
                .max_depth
                .is_some_and(|maximum| depth + 1 > maximum)
            || !policy.path_can_be_scanned(&relative_display, &name, false, &ignore_rules)
        {
            continue;
        }
        if let Some(file) = read_file_info(
            policy,
            &absolute_path,
            relative_path,
            known_files,
            diagnostics,
            control,
        )? {
            files.push(file);
        }
    }
    Ok(())
}

fn read_file_info(
    policy: &RootPolicy,
    absolute_path: &Path,
    relative_path: &Path,
    known_files: &HashSet<KnownFileKey>,
    diagnostics: &mut ScanDiagnostics,
    control: &BlockingControl,
) -> Result<Option<DiscoveredFile>, HostError> {
    control.check()?;
    let Ok(metadata) = fs::metadata(absolute_path) else {
        return Ok(None);
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() == 0 {
        record_skipped(
            diagnostics,
            absolute_path,
            SkippedFileReason::Empty,
            Some(0),
            None,
        );
        return Ok(None);
    }
    let Some(detected) = detect_file_type(absolute_path) else {
        record_skipped(
            diagnostics,
            absolute_path,
            SkippedFileReason::Unsupported,
            Some(metadata.len()),
            None,
        );
        return Ok(None);
    };
    let maximum = max_file_size(detected.kind, policy.root().discovery.max_file_size_bytes);
    if metadata.len() > maximum {
        record_skipped(
            diagnostics,
            absolute_path,
            SkippedFileReason::TooLarge,
            Some(metadata.len()),
            Some(maximum),
        );
        return Ok(None);
    }
    let modified_epoch_ms = modified_epoch_ms(&metadata);
    let source_fingerprint = source_fingerprint(metadata.len(), modified_epoch_ms);
    let known = KnownFileKey {
        root: policy.root_path().to_path_buf(),
        relative_path: relative_path.to_path_buf(),
        source_fingerprint: source_fingerprint.clone(),
    };
    if detected.kind != FileKind::Image
        && !known_files.contains(&known)
        && is_likely_binary_file(absolute_path)
    {
        record_skipped(
            diagnostics,
            absolute_path,
            SkippedFileReason::Binary,
            Some(metadata.len()),
            None,
        );
        return Ok(None);
    }
    Ok(Some(DiscoveredFile {
        root: policy.root_path().to_path_buf(),
        relative_path: relative_path.to_path_buf(),
        size_bytes: metadata.len(),
        modified_epoch_ms,
        source_fingerprint,
        kind_hint: Some(detected.kind),
        format_hint: Some(detected.format),
    }))
}

fn read_batch_sync(
    request: &ReadBatchRequest,
    control: &BlockingControl,
) -> Result<Vec<SourceFile>, HostError> {
    let mut sources = Vec::with_capacity(request.files.len());
    for file in &request.files {
        control.check()?;
        validate_relative_path(&file.relative_path)?;
        let absolute_path = if fs::metadata(&file.root).is_ok_and(|metadata| metadata.is_file()) {
            file.root.clone()
        } else {
            file.root.join(&file.relative_path)
        };
        let bytes = fs::read(&absolute_path).map_err(|error| {
            HostError::backend(
                "native-scanner",
                format!("could not read source {}: {error}", absolute_path.display()),
            )
        })?;
        let metadata = fs::metadata(&absolute_path).map_err(|error| {
            HostError::backend(
                "native-scanner",
                format!(
                    "could not inspect source {}: {error}",
                    absolute_path.display()
                ),
            )
        })?;
        sources.push(SourceFile {
            root: file.root.clone(),
            relative_path: file.relative_path.clone(),
            bytes,
            source_fingerprint: source_fingerprint(metadata.len(), modified_epoch_ms(&metadata)),
            kind_hint: file.kind_hint,
            format_hint: file.format_hint.clone(),
        });
    }
    Ok(sources)
}

fn validate_relative_path(path: &Path) -> Result<(), HostError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(HostError::invalid_input(format!(
            "source path must be a non-empty relative path: {}",
            path.display()
        )));
    }
    Ok(())
}

fn modified_epoch_ms(metadata: &Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn source_fingerprint(size_bytes: u64, modified_epoch_ms: Option<u64>) -> String {
    modified_epoch_ms.map_or_else(
        || format!("metadata-v1:{size_bytes}:unknown"),
        |modified| format!("metadata-v1:{size_bytes}:{modified}"),
    )
}

fn is_likely_binary_file(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut buffer = [0_u8; BINARY_SNIFF_BYTES];
    let Ok(bytes_read) = file.read(&mut buffer) else {
        return false;
    };
    if bytes_read == 0 {
        return false;
    }
    let mut suspicious = 0_usize;
    for value in &buffer[..bytes_read] {
        if *value == 0 {
            return true;
        }
        if is_suspicious_control_byte(*value) {
            suspicious += 1;
        }
    }
    suspicious * 100 > bytes_read * BINARY_CONTROL_CHAR_PERCENT
}

fn is_suspicious_control_byte(value: u8) -> bool {
    value < 32 && !matches!(value, 7 | 8 | 9 | 10 | 12 | 13 | 27)
}

fn record_skipped(
    diagnostics: &mut ScanDiagnostics,
    path: &Path,
    reason: SkippedFileReason,
    size_bytes: Option<u64>,
    limit_bytes: Option<u64>,
) {
    diagnostics.skipped_files += 1;
    match reason {
        SkippedFileReason::Empty => diagnostics.skipped_by_reason.empty += 1,
        SkippedFileReason::TooLarge => diagnostics.skipped_by_reason.too_large += 1,
        SkippedFileReason::Unsupported => diagnostics.skipped_by_reason.unsupported += 1,
        SkippedFileReason::Binary => diagnostics.skipped_by_reason.binary += 1,
    }
    if diagnostics.skipped_samples.len() < MAX_SKIPPED_FILE_SAMPLES {
        diagnostics.skipped_samples.push(SkippedFile {
            path: path.to_path_buf(),
            reason,
            size_bytes,
            limit_bytes,
        });
    }
}

fn is_nested_git_repository_directory(path: &Path) -> bool {
    fs::metadata(path.join(".git")).is_ok_and(|metadata| metadata.is_file() || metadata.is_dir())
}
