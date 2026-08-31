use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use futures_util::{StreamExt, stream::FuturesUnordered};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use zg_host_native::{
    DiscoveredFile, DiscoveryOptions as HostDiscoveryOptions, FileKind as HostFileKind, HostError,
    KnownSourceFile, ReadBatchRequest, RootSpec, ScanRequest, ScanSnapshot,
    SourceFile as HostSource, TaskControl, WorkspaceScannerPort,
};

use crate::{
    EngineError,
    api::{
        index::{
            options::{RootPath, WorkspaceChange},
            progress::{
                IndexEmbeddingProgress, IndexProgress, IndexProgressPhase, IndexProgressReporter,
            },
            result::{IndexResult, SkippedFile, SkippedFileReason, TimingEntry},
        },
        info::result::{WorkspaceIndexInfo, WorkspaceIndexPolicy, WorkspaceIndexStatus},
    },
    extraction::{
        FileKind, ImageSource, IndexingExtractionFragment, SourceFile, TextSource,
        extract_for_indexing, vector_content_for_fragment,
    },
    models::{
        EmbeddingInputKind, EmbeddingModelInfo, EmbeddingOptions, EmbeddingPurpose,
        EmbeddingResult, ModelError, ModelRuntimeLease,
    },
    payload::{Content, ImageFormat},
    storage::spi::{FileIndexDiagnostics, FileInfo, IndexedFragment, WorkspaceIndexStorage},
};

use super::input_budget::index_chunk_options;

const EMBEDDING_TRANSIENT_MAX_RETRIES: usize = 3;
const EMBEDDING_RATE_LIMIT_MAX_RETRIES: usize = 6;
const EMBEDDING_TRANSIENT_RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
const EMBEDDING_RATE_LIMIT_RETRY_BASE_DELAY: Duration = Duration::from_secs(2);
const EMBEDDING_TRANSIENT_RETRY_MAX_DELAY: Duration = Duration::from_secs(8);
const EMBEDDING_RATE_LIMIT_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);
const EMBEDDING_RETRY_JITTER_MILLIS: u64 = 500;
const EMBEDDING_SUCCESS_STREAK_MIN: usize = 4;

#[async_trait]
pub(crate) trait IndexEmbeddingRuntime: Send + Sync {
    fn info(&self) -> &EmbeddingModelInfo;

    async fn embed(
        &self,
        contents: &[Content],
        options: EmbeddingOptions,
        progress: Option<IndexProgressReporter>,
    ) -> Result<EmbeddingResult, ModelError>;
}

#[async_trait]
impl IndexEmbeddingRuntime for ModelRuntimeLease {
    fn info(&self) -> &EmbeddingModelInfo {
        self.info()
    }

    async fn embed(
        &self,
        contents: &[Content],
        options: EmbeddingOptions,
        progress: Option<IndexProgressReporter>,
    ) -> Result<EmbeddingResult, ModelError> {
        self.embed(contents, options, progress).await
    }
}

pub(crate) struct IndexingContext<'context> {
    pub workspace_index: &'context WorkspaceIndexInfo,
    pub storage: &'context dyn WorkspaceIndexStorage,
    pub scanner: &'context dyn WorkspaceScannerPort,
    pub embedding_model: &'context dyn IndexEmbeddingRuntime,
    pub embedding_concurrency: Option<usize>,
    pub on_progress: Option<IndexProgressReporter>,
    pub signal: Option<CancellationToken>,
    pub changes: &'context [WorkspaceChange],
}

pub(crate) async fn index_workspace(
    context: &IndexingContext<'_>,
) -> Result<IndexResult, EngineError> {
    validate_context(context)?;
    let started = Instant::now();
    let mut timings = TimingCollector::default();

    let first = run_index_pass(context, &mut timings, None).await?;
    let mut passes = vec![first];
    if passes[0].stats.files_failed > 0 {
        let succeeded = passes[0].stats.files_indexed;
        let files_total = passes[0].diff.pending_count();
        report(
            context,
            IndexProgress {
                phase: IndexProgressPhase::Scanning,
                files_total: Some(files_total),
                files_indexed: Some(succeeded),
                files_failed: Some(passes[0].stats.files_failed),
                detail: Some(format!(
                    "retrying {} failed files",
                    passes[0].stats.files_failed
                )),
                embedding: None,
            },
        );
        passes.push(
            run_index_pass(
                context,
                &mut timings,
                Some(ProgressBase {
                    files_succeeded: succeeded,
                    files_total,
                }),
            )
            .await?,
        );
    }

    throw_if_cancelled(context.signal.as_ref())?;
    let final_pass = passes.last().expect("an index pass is always present");
    report(
        context,
        IndexProgress {
            phase: IndexProgressPhase::Indexing,
            files_total: Some(final_pass.diff.pending_count()),
            files_indexed: Some(final_pass.stats.files_indexed + final_pass.stats.files_failed),
            files_failed: Some(final_pass.stats.files_failed),
            detail: Some("finalizing index".to_owned()),
            embedding: None,
        },
    );
    let finalize_started = Instant::now();
    context
        .storage
        .finalize_writes()
        .await
        .map_err(|error| EngineError::backend("indexing", format!("finalize storage: {error}")))?;
    timings.record("index_optimize", finalize_started.elapsed(), 1);

    let result = build_index_result(context, &passes, started.elapsed(), timings);
    if result.files_failed > 0 {
        report(
            context,
            IndexProgress {
                phase: IndexProgressPhase::Done,
                files_total: Some(final_pass.diff.pending_count()),
                files_indexed: Some(final_pass.stats.files_indexed),
                files_failed: Some(result.files_failed),
                detail: Some("indexing completed with failed files".to_owned()),
                embedding: None,
            },
        );
        return Err(EngineError::backend(
            "indexing",
            format!(
                "indexing completed with {} failed files: {}",
                result.files_failed,
                final_pass
                    .stats
                    .failed_files
                    .iter()
                    .take(5)
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }

    report(
        context,
        IndexProgress {
            phase: IndexProgressPhase::Done,
            files_total: Some(final_pass.diff.pending_count()),
            files_indexed: Some(final_pass.stats.files_indexed),
            files_failed: Some(0),
            detail: Some("indexing complete".to_owned()),
            embedding: None,
        },
    );
    Ok(result)
}

pub(crate) async fn get_workspace_index_status(
    workspace_index: &WorkspaceIndexInfo,
    storage: &dyn WorkspaceIndexStorage,
    scanner: &dyn WorkspaceScannerPort,
    signal: Option<CancellationToken>,
) -> Result<WorkspaceIndexStatus, EngineError> {
    let stored_files = storage.list_files()?;
    let control = task_control(signal);
    let snapshot = scanner
        .discover(
            &ScanRequest {
                roots: host_roots(&workspace_index.roots),
                known_files: known_source_files(&stored_files),
            },
            &control,
        )
        .await
        .map_err(map_host_error)?;
    let discovered_files = scanned_files(&workspace_index.id, snapshot.files)?;
    let mut diff = compute_diff(discovered_files, &stored_files);
    resolve_status_modifications(scanner, &control, &mut diff).await?;

    let pending_files = stored_files
        .iter()
        .filter(|file| {
            file.index_status
                .as_ref()
                .is_some_and(|status| status.indexed_epoch_ms.is_none())
        })
        .collect::<Vec<_>>();
    let indexed_files = stored_files
        .iter()
        .filter(|file| {
            file.index_status
                .as_ref()
                .is_some_and(|status| status.indexed_epoch_ms.is_some())
        })
        .collect::<Vec<_>>();

    Ok(WorkspaceIndexStatus {
        files_scanned: diff.files_scanned,
        files_stored: stored_files.len(),
        files_indexed: indexed_files.len(),
        entities_indexed: indexed_files
            .iter()
            .map(|file| {
                file.index_status
                    .as_ref()
                    .map_or(0, |status| status.entity_count)
            })
            .sum(),
        fragments_truncated: indexed_files
            .iter()
            .map(|file| {
                file.index_status
                    .as_ref()
                    .and_then(|status| status.truncated_fragment_count)
                    .unwrap_or(0)
            })
            .sum(),
        files_pending: pending_files.len(),
        files_failed: pending_files
            .iter()
            .filter(|file| {
                file.index_status
                    .as_ref()
                    .is_some_and(|status| status.error.is_some())
            })
            .count(),
        files_added: diff.added,
        files_modified: diff.modified,
        files_deleted: diff.deleted.len(),
        files_unchanged: diff.unchanged,
    })
}

#[derive(Clone, Copy)]
struct ProgressBase {
    files_succeeded: usize,
    files_total: usize,
}

struct IndexPassResult {
    files_scanned: usize,
    diff: DiffPlan,
    stats: IndexStats,
    skipped: Vec<SkippedFile>,
}

#[derive(Default)]
struct IndexStats {
    files_indexed: usize,
    files_failed: usize,
    entities_created: usize,
    failed_files: Vec<PathBuf>,
    failed_reasons: Vec<String>,
}

#[derive(Default)]
struct DiffPlan {
    files_scanned: usize,
    added: usize,
    modified: usize,
    pending: usize,
    unchanged: usize,
    deleted: Vec<FileInfo>,
    candidates: Vec<IndexCandidate>,
}

impl DiffPlan {
    const fn pending_count(&self) -> usize {
        self.added + self.modified + self.pending
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CandidateKind {
    Added,
    Modified,
    Pending,
}

struct IndexCandidate {
    kind: CandidateKind,
    file: FileInfo,
    discovered: DiscoveredFile,
    existing: Option<FileInfo>,
}

struct ScannedFile {
    file: FileInfo,
    discovered: DiscoveredFile,
}

async fn run_index_pass(
    context: &IndexingContext<'_>,
    timings: &mut TimingCollector,
    progress_base: Option<ProgressBase>,
) -> Result<IndexPassResult, EngineError> {
    throw_if_cancelled(context.signal.as_ref())?;
    report(
        context,
        IndexProgress {
            phase: IndexProgressPhase::Scanning,
            files_total: progress_base.map(|base| base.files_total),
            files_indexed: progress_base.map(|base| base.files_succeeded),
            files_failed: None,
            detail: Some(if progress_base.is_some() {
                "scanning retry candidates".to_owned()
            } else if context.changes.is_empty() {
                "scanning files".to_owned()
            } else {
                "scanning changed paths".to_owned()
            }),
            embedding: None,
        },
    );

    let scope = ChangeScope::from_changes(&context.workspace_index.roots, context.changes);
    let all_stored = context.storage.list_files()?;
    let existing = scope.filter_stored(&all_stored);
    let scan_started = Instant::now();
    let control = task_control(context.signal.clone());
    let snapshot = context
        .scanner
        .discover(
            &ScanRequest {
                roots: host_roots(&context.workspace_index.roots),
                known_files: known_source_files(&all_stored),
            },
            &control,
        )
        .await
        .map_err(map_host_error)?;
    let skipped = skipped_files(&snapshot);
    let scanned = scope.filter_scanned(scanned_files(&context.workspace_index.id, snapshot.files)?);
    timings.record("index_scan", scan_started.elapsed(), scanned.len());

    let diff_started = Instant::now();
    let mut diff = compute_diff(scanned, &existing);
    timings.record("index_diff", diff_started.elapsed(), diff.files_scanned);
    report(
        context,
        IndexProgress {
            phase: IndexProgressPhase::Scanning,
            files_total: Some(progress_base.map_or(diff.candidates.len(), |base| base.files_total)),
            files_indexed: Some(progress_base.map_or(0, |base| base.files_succeeded)),
            files_failed: None,
            detail: Some(format!(
                "{} candidates, {} deleted, {} unchanged",
                diff.candidates.len(),
                diff.deleted.len(),
                diff.unchanged
            )),
            embedding: None,
        },
    );

    let delete_started = Instant::now();
    for file in &diff.deleted {
        throw_if_cancelled(context.signal.as_ref())?;
        context.storage.delete_file(&file.id).map_err(|error| {
            EngineError::backend(
                "indexing",
                format!(
                    "delete stale file {}: {error}",
                    file.relative_path.display()
                ),
            )
        })?;
    }
    timings.record(
        "index_delete_stale",
        delete_started.elapsed(),
        diff.deleted.len(),
    );

    let stats = index_candidates(context, &control, &mut diff, timings, progress_base).await?;
    Ok(IndexPassResult {
        files_scanned: diff.files_scanned,
        diff,
        stats,
        skipped,
    })
}

fn compute_diff(scanned: Vec<ScannedFile>, existing_files: &[FileInfo]) -> DiffPlan {
    let existing_by_id = existing_files
        .iter()
        .cloned()
        .map(|file| (file.id.clone(), file))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut plan = DiffPlan {
        files_scanned: scanned.len(),
        ..DiffPlan::default()
    };

    for scanned in scanned {
        seen.insert(scanned.file.id.clone());
        let existing = existing_by_id.get(&scanned.file.id).cloned();
        let kind = match &existing {
            None => CandidateKind::Added,
            Some(existing)
                if existing
                    .index_status
                    .as_ref()
                    .is_some_and(|status| status.indexed_epoch_ms.is_none()) =>
            {
                CandidateKind::Pending
            }
            Some(existing)
                if existing.size_bytes == scanned.file.size_bytes
                    && existing.modified_epoch_ms == scanned.file.modified_epoch_ms
                    && existing.content_hash.is_some() =>
            {
                plan.unchanged += 1;
                continue;
            }
            Some(_) => CandidateKind::Modified,
        };
        plan.increment(kind);
        plan.candidates.push(IndexCandidate {
            kind,
            file: scanned.file,
            discovered: scanned.discovered,
            existing,
        });
    }
    plan.deleted = existing_by_id
        .into_values()
        .filter(|file| !seen.contains(&file.id))
        .collect();
    plan.deleted
        .sort_by(|left, right| left.absolute_path.cmp(&right.absolute_path));
    plan
}

impl DiffPlan {
    fn increment(&mut self, kind: CandidateKind) {
        match kind {
            CandidateKind::Added => self.added += 1,
            CandidateKind::Modified => self.modified += 1,
            CandidateKind::Pending => self.pending += 1,
        }
    }

    fn resolve_modified_as_unchanged(&mut self) {
        self.modified = self.modified.saturating_sub(1);
        self.unchanged += 1;
    }
}

async fn resolve_status_modifications(
    scanner: &dyn WorkspaceScannerPort,
    control: &TaskControl,
    diff: &mut DiffPlan,
) -> Result<(), EngineError> {
    let mut same_content = 0;
    for candidate in &diff.candidates {
        if candidate.kind != CandidateKind::Modified {
            continue;
        }
        let source = read_source(scanner, control, &candidate.discovered).await?;
        let hash = sha256_bytes(&source.bytes);
        if candidate.existing.as_ref().is_some_and(|existing| {
            existing.size_bytes == candidate.file.size_bytes
                && existing.content_hash.as_deref() == Some(hash.as_str())
        }) {
            same_content += 1;
        }
    }
    diff.modified = diff.modified.saturating_sub(same_content);
    diff.unchanged += same_content;
    Ok(())
}

struct PreparedFragment {
    fragment: crate::extraction::EntityFragment,
    embedding_content: Content,
}

struct PreparedFile {
    file: FileInfo,
    fragments: Vec<PreparedFragment>,
}

enum PreparedCandidate {
    File(Box<PreparedFile>),
    Unchanged,
}

type EmbeddingFuture<'context> =
    Pin<Box<dyn Future<Output = EmbeddingBatchOutcome> + Send + 'context>>;

#[expect(
    clippy::too_many_lines,
    reason = "the bounded prepare/schedule/drain loop is clearest as one linear orchestration"
)]
async fn index_candidates(
    context: &IndexingContext<'_>,
    control: &TaskControl,
    diff: &mut DiffPlan,
    timings: &mut TimingCollector,
    progress_base: Option<ProgressBase>,
) -> Result<IndexStats, EngineError> {
    let policy = resolve_embedding_policy(
        context.embedding_concurrency,
        context.embedding_model.info(),
    )?;
    let scheduler = Arc::new(EmbeddingScheduler::new(policy));
    let max_batch_size = context.embedding_model.info().limits.max_batch_size;
    let mut stats = IndexStats::default();
    let mut current_batch = Vec::new();
    let mut current_fragments = 0;
    let mut running: FuturesUnordered<EmbeddingFuture<'_>> = FuturesUnordered::new();

    report_indexing(
        context,
        &stats,
        diff,
        progress_base,
        None,
        Some(scheduler.snapshot()),
    );

    let candidates = std::mem::take(&mut diff.candidates);
    for candidate in candidates {
        throw_if_cancelled(context.signal.as_ref())?;
        report_indexing(
            context,
            &stats,
            diff,
            progress_base,
            Some(format!(
                "reading {}",
                candidate.file.relative_path.display()
            )),
            None,
        );
        let prepare_started = Instant::now();
        let prepared = prepare_candidate(context, control, &candidate).await;
        timings.record("index_prepare", prepare_started.elapsed(), 1);
        let prepared = match prepared {
            Ok(PreparedCandidate::Unchanged) => {
                diff.resolve_modified_as_unchanged();
                continue;
            }
            Ok(PreparedCandidate::File(prepared)) => *prepared,
            Err(error) => {
                let reason = mark_file_failed(context.storage, &candidate.file, "prepare", &error)?;
                record_file_failed(&mut stats, &candidate.file, &reason);
                report_indexing(
                    context,
                    &stats,
                    diff,
                    progress_base,
                    Some(format!("failed {}", candidate.file.relative_path.display())),
                    None,
                );
                continue;
            }
        };

        if prepared.fragments.is_empty() {
            let commit_started = Instant::now();
            if let Err(error) = commit_file(
                context.storage,
                prepared,
                Vec::new(),
                Vec::new(),
                &mut stats,
            ) {
                let reason =
                    mark_file_failed(context.storage, &error.file, "commit", &error.error)?;
                record_file_failed(&mut stats, &error.file, &reason);
            }
            timings.record("index_commit", commit_started.elapsed(), 1);
            report_indexing(context, &stats, diff, progress_base, None, None);
            continue;
        }

        if prepared.fragments.len() > max_batch_size {
            if !current_batch.is_empty() {
                push_embedding(
                    &mut running,
                    std::mem::take(&mut current_batch),
                    context,
                    Arc::clone(&scheduler),
                );
                current_fragments = 0;
            }
            push_embedding(
                &mut running,
                vec![prepared],
                context,
                Arc::clone(&scheduler),
            );
        } else {
            if current_fragments > 0
                && current_fragments + prepared.fragments.len() > max_batch_size
            {
                push_embedding(
                    &mut running,
                    std::mem::take(&mut current_batch),
                    context,
                    Arc::clone(&scheduler),
                );
                current_fragments = 0;
            }
            current_fragments += prepared.fragments.len();
            current_batch.push(prepared);
            if current_fragments == max_batch_size {
                push_embedding(
                    &mut running,
                    std::mem::take(&mut current_batch),
                    context,
                    Arc::clone(&scheduler),
                );
                current_fragments = 0;
            }
        }

        if running.len() >= scheduler.task_concurrency()
            && let Some(outcome) = running.next().await
        {
            apply_embedding_outcome(context, diff, progress_base, timings, &mut stats, outcome)?;
        }
    }

    if !current_batch.is_empty() {
        push_embedding(&mut running, current_batch, context, Arc::clone(&scheduler));
    }
    while let Some(outcome) = running.next().await {
        apply_embedding_outcome(context, diff, progress_base, timings, &mut stats, outcome)?;
    }
    throw_if_cancelled(context.signal.as_ref())?;
    Ok(stats)
}

fn push_embedding<'context>(
    running: &mut FuturesUnordered<EmbeddingFuture<'context>>,
    files: Vec<PreparedFile>,
    context: &'context IndexingContext<'context>,
    scheduler: Arc<EmbeddingScheduler>,
) {
    running.push(Box::pin(embed_prepared_files(
        files,
        context.embedding_model,
        scheduler,
        context.signal.clone(),
        context.on_progress.clone(),
    )));
}

fn apply_embedding_outcome(
    context: &IndexingContext<'_>,
    diff: &DiffPlan,
    progress_base: Option<ProgressBase>,
    timings: &mut TimingCollector,
    stats: &mut IndexStats,
    outcome: EmbeddingBatchOutcome,
) -> Result<(), EngineError> {
    timings.record("index_embedding", outcome.duration, outcome.outcomes.len());
    for outcome in outcome.outcomes {
        throw_if_cancelled(context.signal.as_ref())?;
        match outcome {
            EmbeddedFileOutcome::Success {
                file,
                vectors,
                truncated,
            } => {
                let path = file.file.relative_path.clone();
                let commit_started = Instant::now();
                if let Err(error) = commit_file(context.storage, file, vectors, truncated, stats) {
                    let file = error.file;
                    let reason = mark_file_failed(context.storage, &file, "commit", &error.error)?;
                    record_file_failed(stats, &file, &reason);
                }
                timings.record("index_commit", commit_started.elapsed(), 1);
                report_indexing(
                    context,
                    stats,
                    diff,
                    progress_base,
                    Some(format!("indexed {}", path.display())),
                    None,
                );
            }
            EmbeddedFileOutcome::Failed { file, reason } => {
                let reason = mark_file_failed(
                    context.storage,
                    &file.file,
                    "embed",
                    &EngineError::backend("embedding", reason),
                )?;
                record_file_failed(stats, &file.file, &reason);
                report_indexing(
                    context,
                    stats,
                    diff,
                    progress_base,
                    Some(format!("failed {}", file.file.relative_path.display())),
                    None,
                );
            }
        }
    }
    Ok(())
}

struct CommitError {
    file: Box<FileInfo>,
    error: EngineError,
}

fn commit_file(
    storage: &dyn WorkspaceIndexStorage,
    file: PreparedFile,
    vectors: Vec<Vec<f32>>,
    truncated: Vec<usize>,
    stats: &mut IndexStats,
) -> Result<(), CommitError> {
    if file.fragments.len() != vectors.len() {
        return Err(CommitError {
            file: Box::new(file.file),
            error: EngineError::backend(
                "storage",
                format!(
                    "entity/vector count mismatch: fragments={} vectors={}",
                    file.fragments.len(),
                    vectors.len()
                ),
            ),
        });
    }
    let truncated = truncated.into_iter().collect::<HashSet<_>>();
    let truncated_fragment_count = file
        .fragments
        .iter()
        .enumerate()
        .filter(|(index, _)| truncated.contains(index))
        .count();
    let public_entities = count_public_entities(&file.fragments);
    let entries = file
        .fragments
        .into_iter()
        .zip(vectors)
        .map(|(fragment, vector)| IndexedFragment {
            fragment: fragment.fragment,
            vector,
        })
        .collect::<Vec<_>>();
    storage
        .replace_file(
            &file.file,
            &entries,
            Some(&FileIndexDiagnostics {
                truncated_fragment_count: Some(truncated_fragment_count),
            }),
        )
        .map_err(|error| CommitError {
            file: Box::new(file.file.clone()),
            error,
        })?;
    stats.files_indexed += 1;
    stats.entities_created += public_entities;
    Ok(())
}

async fn prepare_candidate(
    context: &IndexingContext<'_>,
    control: &TaskControl,
    candidate: &IndexCandidate,
) -> Result<PreparedCandidate, EngineError> {
    let source = read_source(context.scanner, control, &candidate.discovered).await?;
    if source.source_fingerprint != candidate.discovered.source_fingerprint {
        return Err(EngineError::backend(
            "indexing",
            format!(
                "source changed while being indexed: {}",
                candidate.file.absolute_path.display()
            ),
        ));
    }

    let mut file = candidate.file.clone();
    file.content_hash = Some(sha256_bytes(&source.bytes));
    if candidate.kind == CandidateKind::Modified
        && candidate.existing.as_ref().is_some_and(|existing| {
            existing.size_bytes == file.size_bytes && existing.content_hash == file.content_hash
        })
    {
        return Ok(PreparedCandidate::Unchanged);
    }

    let extraction_file = extraction_file(&file);
    let chunk_options = index_chunk_options(
        context.embedding_model.info().limits.max_input_tokens,
        (file.kind != FileKind::Image)
            .then(|| String::from_utf8_lossy(&source.bytes))
            .as_deref(),
    );
    let extracted = if file.kind == FileKind::Image {
        let image = ImageSource {
            file: extraction_file,
            data: source.bytes,
            format: image_format(&file.format)?,
        };
        extract_for_indexing(&image, chunk_options)?
    } else {
        let text = TextSource {
            file: extraction_file,
            text: String::from_utf8_lossy(&source.bytes).into_owned(),
        };
        extract_for_indexing(&text, chunk_options)?
    };
    let fragments = prepare_fragments(
        extracted,
        context.embedding_model.info(),
        chunk_options.max_chunk_chars,
    );
    Ok(PreparedCandidate::File(Box::new(PreparedFile {
        file,
        fragments,
    })))
}

fn prepare_fragments(
    extracted: Vec<IndexingExtractionFragment>,
    model: &EmbeddingModelInfo,
    max_chars: Option<usize>,
) -> Vec<PreparedFragment> {
    extracted
        .into_iter()
        .filter(|item| content_supported(model, &item.fragment.content))
        .map(|item| PreparedFragment {
            embedding_content: vector_content_for_fragment(
                &item.fragment,
                item.embedding_source.as_ref(),
                max_chars,
            ),
            fragment: item.fragment,
        })
        .collect()
}

fn content_supported(model: &EmbeddingModelInfo, content: &Content) -> bool {
    let kind = match content {
        Content::Text(_) => EmbeddingInputKind::Text,
        Content::Image(_) => EmbeddingInputKind::Image,
    };
    model.input_kinds.contains(&kind)
}

fn extraction_file(file: &FileInfo) -> SourceFile {
    SourceFile {
        id: file.id.clone(),
        absolute_path: file.absolute_path.clone(),
        relative_path: file.relative_path.clone(),
        root_path: file.root_path.clone(),
        size_bytes: file.size_bytes,
        modified_epoch_ms: Some(file.modified_epoch_ms),
        content_hash: file.content_hash.clone(),
        kind: file.kind,
        format: file.format.clone(),
    }
}

fn count_public_entities(fragments: &[PreparedFragment]) -> usize {
    fragments
        .iter()
        .filter(|item| {
            item.fragment
                .group
                .as_ref()
                .is_none_or(|group| group == &item.fragment.id)
        })
        .count()
}

fn mark_file_failed(
    storage: &dyn WorkspaceIndexStorage,
    file: &FileInfo,
    stage: &str,
    error: &EngineError,
) -> Result<String, EngineError> {
    let reason = one_line(&format!("{stage}: {error}"));
    storage
        .mark_file_failed(file, &reason)
        .map_err(|mark_error| {
            EngineError::backend(
                "indexing",
                format!(
                    "record failure for {}: {mark_error}; original={reason}",
                    file.relative_path.display()
                ),
            )
        })?;
    Ok(reason)
}

fn record_file_failed(stats: &mut IndexStats, file: &FileInfo, reason: &str) {
    stats.files_failed += 1;
    stats.failed_files.push(file.relative_path.clone());
    stats
        .failed_reasons
        .push(format!("{}: {reason}", file.relative_path.display()));
}

struct EmbeddingBatchOutcome {
    outcomes: Vec<EmbeddedFileOutcome>,
    duration: Duration,
}

enum EmbeddedFileOutcome {
    Success {
        file: PreparedFile,
        vectors: Vec<Vec<f32>>,
        truncated: Vec<usize>,
    },
    Failed {
        file: PreparedFile,
        reason: String,
    },
}

async fn embed_prepared_files(
    files: Vec<PreparedFile>,
    model: &dyn IndexEmbeddingRuntime,
    scheduler: Arc<EmbeddingScheduler>,
    signal: Option<CancellationToken>,
    progress: Option<IndexProgressReporter>,
) -> EmbeddingBatchOutcome {
    let started = Instant::now();
    if files.len() == 1 && files[0].fragments.len() > model.info().limits.max_batch_size {
        let file = files.into_iter().next().expect("one prepared file");
        let outcome = match embed_file(&file, model, &scheduler, signal.as_ref(), progress).await {
            Ok(embedding) => EmbeddedFileOutcome::Success {
                file,
                vectors: embedding.vectors,
                truncated: embedding.truncated,
            },
            Err(error) => EmbeddedFileOutcome::Failed {
                file,
                reason: model_error_text(&error),
            },
        };
        return EmbeddingBatchOutcome {
            outcomes: vec![outcome],
            duration: started.elapsed(),
        };
    }
    let contents = files
        .iter()
        .flat_map(|file| {
            file.fragments
                .iter()
                .map(|fragment| fragment.embedding_content.clone())
        })
        .collect::<Vec<_>>();
    report_embedding_progress(progress.as_ref(), &scheduler, describe_files(&files));
    let result = embed_with_retry(
        model,
        &contents,
        &scheduler,
        signal.as_ref(),
        progress.clone(),
    )
    .await;
    let outcomes = match result {
        Ok(embedding) => split_embedding(files, embedding),
        Err(error) if classify_embedding_retry(&error).retryable => {
            let reason = model_error_text(&error);
            files
                .into_iter()
                .map(|file| EmbeddedFileOutcome::Failed {
                    file,
                    reason: reason.clone(),
                })
                .collect()
        }
        Err(_) => {
            let mut outcomes = Vec::with_capacity(files.len());
            for file in files {
                match embed_file(&file, model, &scheduler, signal.as_ref(), progress.clone()).await
                {
                    Ok(embedding) => outcomes.push(EmbeddedFileOutcome::Success {
                        file,
                        vectors: embedding.vectors,
                        truncated: embedding.truncated,
                    }),
                    Err(error) => outcomes.push(EmbeddedFileOutcome::Failed {
                        file,
                        reason: model_error_text(&error),
                    }),
                }
            }
            outcomes
        }
    };
    EmbeddingBatchOutcome {
        outcomes,
        duration: started.elapsed(),
    }
}

fn split_embedding(
    files: Vec<PreparedFile>,
    embedding: EmbeddingResult,
) -> Vec<EmbeddedFileOutcome> {
    if embedding.vectors.len() != files.iter().map(|file| file.fragments.len()).sum::<usize>() {
        let reason = format!(
            "embedding returned {} vectors for {} fragments",
            embedding.vectors.len(),
            files.iter().map(|file| file.fragments.len()).sum::<usize>()
        );
        return files
            .into_iter()
            .map(|file| EmbeddedFileOutcome::Failed {
                file,
                reason: reason.clone(),
            })
            .collect();
    }

    let truncated = embedding.truncated.into_iter().collect::<HashSet<_>>();
    let mut vectors = embedding.vectors.into_iter();
    let mut offset = 0;
    files
        .into_iter()
        .map(|file| {
            let count = file.fragments.len();
            let file_vectors = vectors.by_ref().take(count).collect::<Vec<_>>();
            let file_truncated = (0..count)
                .filter(|index| truncated.contains(&(offset + index)))
                .collect::<Vec<_>>();
            offset += count;
            EmbeddedFileOutcome::Success {
                file,
                vectors: file_vectors,
                truncated: file_truncated,
            }
        })
        .collect()
}

async fn embed_file(
    file: &PreparedFile,
    model: &dyn IndexEmbeddingRuntime,
    scheduler: &EmbeddingScheduler,
    signal: Option<&CancellationToken>,
    progress: Option<IndexProgressReporter>,
) -> Result<EmbeddingResult, ModelError> {
    let maximum = model.info().limits.max_batch_size;
    let mut running = FuturesUnordered::new();
    for (batch_index, fragments) in file.fragments.chunks(maximum).enumerate() {
        running.push(embed_fragment_batch(
            batch_index * maximum,
            fragments,
            model,
            scheduler,
            signal,
            progress.clone(),
        ));
    }

    let mut batches = Vec::new();
    while let Some(result) = running.next().await {
        batches.push(result?);
    }
    batches.sort_by_key(|batch| batch.start);
    let mut vectors = Vec::with_capacity(file.fragments.len());
    let mut truncated = Vec::new();
    for batch in batches {
        vectors.extend(batch.embedding.vectors);
        truncated.extend(
            batch
                .embedding
                .truncated
                .into_iter()
                .map(|index| batch.start + index),
        );
    }
    Ok(EmbeddingResult { vectors, truncated })
}

struct FragmentBatchResult {
    start: usize,
    embedding: EmbeddingResult,
}

async fn embed_fragment_batch(
    start: usize,
    fragments: &[PreparedFragment],
    model: &dyn IndexEmbeddingRuntime,
    scheduler: &EmbeddingScheduler,
    signal: Option<&CancellationToken>,
    progress: Option<IndexProgressReporter>,
) -> Result<FragmentBatchResult, ModelError> {
    let contents = fragments
        .iter()
        .map(|fragment| fragment.embedding_content.clone())
        .collect::<Vec<_>>();
    match embed_with_retry(model, &contents, scheduler, signal, progress.clone()).await {
        Ok(embedding) => Ok(FragmentBatchResult { start, embedding }),
        Err(error) if fragments.len() == 1 || classify_embedding_retry(&error).retryable => {
            Err(error)
        }
        Err(_) => {
            let mut vectors = Vec::with_capacity(fragments.len());
            let mut truncated = Vec::new();
            for (index, fragment) in fragments.iter().enumerate() {
                let embedding = embed_with_retry(
                    model,
                    std::slice::from_ref(&fragment.embedding_content),
                    scheduler,
                    signal,
                    progress.clone(),
                )
                .await
                .map_err(|error| {
                    ModelError::uncoded(format!(
                        "fragment {} failed after one-by-one fallback: {}",
                        fragment.fragment.id,
                        model_error_text(&error)
                    ))
                })?;
                let Some(vector) = embedding.vectors.into_iter().next() else {
                    return Err(ModelError::uncoded(
                        "embedding returned no vector for a fragment",
                    ));
                };
                vectors.push(vector);
                if !embedding.truncated.is_empty() {
                    truncated.push(index);
                }
            }
            Ok(FragmentBatchResult {
                start,
                embedding: EmbeddingResult { vectors, truncated },
            })
        }
    }
}

async fn embed_with_retry(
    model: &dyn IndexEmbeddingRuntime,
    contents: &[Content],
    scheduler: &EmbeddingScheduler,
    signal: Option<&CancellationToken>,
    progress: Option<IndexProgressReporter>,
) -> Result<EmbeddingResult, ModelError> {
    let mut attempt = 0;
    loop {
        if signal.is_some_and(CancellationToken::is_cancelled) {
            return Err(ModelError::uncoded("embedding was cancelled"));
        }
        let permit = scheduler.acquire(signal).await?;
        let result = model
            .embed(
                contents,
                EmbeddingOptions {
                    purpose: Some(EmbeddingPurpose::Document),
                    signal: signal.cloned(),
                    ..EmbeddingOptions::default()
                },
                progress.clone(),
            )
            .await;
        drop(permit);
        match result {
            Ok(result) => {
                scheduler.record_success();
                return Ok(result);
            }
            Err(error) => {
                let retry = classify_embedding_retry(&error);
                if !retry.retryable || attempt >= maximum_retry_attempts(retry) {
                    return Err(error);
                }
                let delay = retry_delay(attempt, retry);
                scheduler.record_retryable_failure(retry.rate_limited, delay);
                abortable_delay(delay, signal).await?;
                attempt += 1;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct RetryClassification {
    retryable: bool,
    rate_limited: bool,
    retry_after: Option<Duration>,
}

fn classify_embedding_retry(error: &ModelError) -> RetryClassification {
    let text = model_error_text(error);
    let normalized = text.to_ascii_lowercase();
    let status = number_after(&normalized, "status=");
    let rate_limited = status == Some(429)
        || normalized.contains("rate limit")
        || normalized.contains("quota exceeded")
        || normalized.contains("too many requests")
        || normalized.contains("request rate increased too quickly");
    let server_error = status.is_some_and(|status| (500..=599).contains(&status));
    let retry_after = number_after(&normalized, "retryafterms=")
        .map(Duration::from_millis)
        .or_else(|| {
            float_after(&normalized, "retryafter=")
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .map(Duration::from_secs_f64)
        });
    RetryClassification {
        retryable: rate_limited || server_error,
        rate_limited,
        retry_after,
    }
}

fn maximum_retry_attempts(retry: RetryClassification) -> usize {
    if retry.rate_limited {
        EMBEDDING_RATE_LIMIT_MAX_RETRIES
    } else {
        EMBEDDING_TRANSIENT_MAX_RETRIES
    }
}

fn retry_delay(attempt: usize, retry: RetryClassification) -> Duration {
    if let Some(retry_after) = retry.retry_after {
        return retry_after;
    }
    let (base, maximum) = if retry.rate_limited {
        (
            EMBEDDING_RATE_LIMIT_RETRY_BASE_DELAY,
            EMBEDDING_RATE_LIMIT_RETRY_MAX_DELAY,
        )
    } else {
        (
            EMBEDDING_TRANSIENT_RETRY_BASE_DELAY,
            EMBEDDING_TRANSIENT_RETRY_MAX_DELAY,
        )
    };
    let multiplier = 1_u32
        .checked_shl(attempt.try_into().unwrap_or(u32::MAX))
        .unwrap_or(u32::MAX);
    let exponential = base.saturating_mul(multiplier);
    exponential
        .saturating_add(Duration::from_millis(pseudo_jitter()))
        .min(maximum)
}

async fn abortable_delay(
    duration: Duration,
    signal: Option<&CancellationToken>,
) -> Result<(), ModelError> {
    if duration.is_zero() {
        return Ok(());
    }
    if let Some(signal) = signal {
        tokio::select! {
            () = tokio::time::sleep(duration) => Ok(()),
            () = signal.cancelled() => Err(ModelError::uncoded("embedding was cancelled")),
        }
    } else {
        tokio::time::sleep(duration).await;
        Ok(())
    }
}

fn model_error_text(error: &ModelError) -> String {
    let mut parts = Vec::new();
    if let Some(code) = error.code() {
        parts.push(code.to_owned());
    }
    parts.push(error.to_string());
    if let Some(context) = error.context() {
        parts.push(context.to_owned());
    }
    if let Some(cause) = error.cause() {
        parts.push(cause.to_owned());
    }
    parts.join(": ")
}

fn number_after(text: &str, marker: &str) -> Option<u64> {
    let start = text.find(marker)? + marker.len();
    let digits = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn float_after(text: &str, marker: &str) -> Option<f64> {
    let start = text.find(marker)? + marker.len();
    let value = text[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    (!value.is_empty()).then(|| value.parse().ok()).flatten()
}

fn pseudo_jitter() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::from(duration.subsec_nanos()) % EMBEDDING_RETRY_JITTER_MILLIS
        })
}

#[derive(Clone, Copy)]
struct EmbeddingConcurrencyPolicy {
    initial: usize,
    minimum: usize,
    maximum: usize,
    adaptive: bool,
}

fn resolve_embedding_policy(
    requested: Option<usize>,
    model: &EmbeddingModelInfo,
) -> Result<EmbeddingConcurrencyPolicy, EngineError> {
    if requested == Some(0) {
        return Err(EngineError::invalid_input(
            "embedding_concurrency must be greater than zero",
        ));
    }
    if let Some(requested) = requested {
        return Ok(EmbeddingConcurrencyPolicy {
            initial: requested,
            minimum: 1,
            maximum: requested,
            adaptive: requested > 1,
        });
    }

    let remote = model.provider == "qwen";
    let multimodal = model.input_kinds.contains(&EmbeddingInputKind::Image);
    let local_default = model.default_concurrency.unwrap_or(1).max(1);
    let initial = if remote {
        if multimodal { 4 } else { 8 }
    } else {
        local_default
    };
    let maximum = if remote {
        if multimodal { 8 } else { 12 }
    } else {
        local_default
    };
    Ok(EmbeddingConcurrencyPolicy {
        initial,
        minimum: initial.min(4),
        maximum: maximum.max(initial),
        adaptive: maximum > 1,
    })
}

struct EmbeddingScheduler {
    policy: EmbeddingConcurrencyPolicy,
    state: Mutex<SchedulerState>,
    notify: Notify,
}

struct SchedulerState {
    active: usize,
    current: usize,
    cooldown_until: Option<Instant>,
    retryable_failures: usize,
    success_streak: usize,
}

impl EmbeddingScheduler {
    fn new(policy: EmbeddingConcurrencyPolicy) -> Self {
        Self {
            policy,
            state: Mutex::new(SchedulerState {
                active: 0,
                current: policy.initial,
                cooldown_until: None,
                retryable_failures: 0,
                success_streak: 0,
            }),
            notify: Notify::new(),
        }
    }

    const fn task_concurrency(&self) -> usize {
        self.policy.maximum
    }

    async fn acquire(
        &self,
        signal: Option<&CancellationToken>,
    ) -> Result<SchedulerPermit<'_>, ModelError> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if signal.is_some_and(CancellationToken::is_cancelled) {
                return Err(ModelError::uncoded(
                    "embedding was cancelled while waiting for capacity",
                ));
            }
            let cooldown = {
                let state = self.lock_state();
                state
                    .cooldown_until
                    .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
            };
            if let Some(duration) = cooldown {
                abortable_delay(duration, signal).await?;
                continue;
            }

            {
                let mut state = self.lock_state();
                if state.active < state.current {
                    state.active += 1;
                    return Ok(SchedulerPermit { scheduler: self });
                }
            }
            if let Some(signal) = signal {
                tokio::select! {
                    () = notified.as_mut() => {}
                    () = signal.cancelled() => {
                        return Err(ModelError::uncoded(
                            "embedding was cancelled while waiting for capacity",
                        ));
                    }
                }
            } else {
                notified.await;
            }
        }
    }

    fn release(&self) {
        let mut state = self.lock_state();
        state.active = state.active.saturating_sub(1);
        drop(state);
        self.notify.notify_waiters();
    }

    fn record_success(&self) {
        let mut state = self.lock_state();
        if !self.policy.adaptive || state.current >= self.policy.maximum {
            return;
        }
        state.success_streak += 1;
        if state.success_streak < EMBEDDING_SUCCESS_STREAK_MIN.max(state.current * 2) {
            return;
        }
        state.current += 1;
        state.success_streak = 0;
        drop(state);
        self.notify.notify_waiters();
    }

    fn record_retryable_failure(&self, rate_limited: bool, delay: Duration) {
        let mut state = self.lock_state();
        state.retryable_failures += 1;
        if rate_limited && !delay.is_zero() {
            let deadline = Instant::now() + delay;
            state.cooldown_until = Some(
                state
                    .cooldown_until
                    .map_or(deadline, |current| current.max(deadline)),
            );
        }
        if self.policy.adaptive {
            state.current = (state.current / 2).max(self.policy.minimum);
            state.success_streak = 0;
        }
    }

    fn snapshot(&self) -> IndexEmbeddingProgress {
        let state = self.lock_state();
        IndexEmbeddingProgress {
            concurrency: Some(state.current),
            max_concurrency: Some(self.policy.maximum),
            retryable_failures: Some(state.retryable_failures),
            ..IndexEmbeddingProgress::default()
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, SchedulerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct SchedulerPermit<'scheduler> {
    scheduler: &'scheduler EmbeddingScheduler,
}

impl Drop for SchedulerPermit<'_> {
    fn drop(&mut self) {
        self.scheduler.release();
    }
}

fn report_embedding_progress(
    reporter: Option<&IndexProgressReporter>,
    scheduler: &EmbeddingScheduler,
    detail: String,
) {
    if let Some(reporter) = reporter {
        reporter.report(IndexProgress {
            phase: IndexProgressPhase::Indexing,
            files_total: None,
            files_indexed: None,
            files_failed: None,
            detail: Some(detail),
            embedding: Some(scheduler.snapshot()),
        });
    }
}

fn report_indexing(
    context: &IndexingContext<'_>,
    stats: &IndexStats,
    diff: &DiffPlan,
    progress_base: Option<ProgressBase>,
    detail: Option<String>,
    embedding: Option<IndexEmbeddingProgress>,
) {
    report(
        context,
        IndexProgress {
            phase: IndexProgressPhase::Indexing,
            files_total: Some(progress_base.map_or(diff.pending_count(), |base| base.files_total)),
            files_indexed: Some(
                progress_base.map_or(0, |base| base.files_succeeded)
                    + stats.files_indexed
                    + stats.files_failed,
            ),
            files_failed: Some(stats.files_failed),
            detail,
            embedding,
        },
    );
}

fn report(context: &IndexingContext<'_>, progress: IndexProgress) {
    if let Some(reporter) = &context.on_progress {
        reporter.report(progress);
    }
}

fn describe_files(files: &[PreparedFile]) -> String {
    match files {
        [] => "embedding 0 files".to_owned(),
        [file] => format!("embedding {}", file.file.relative_path.display()),
        [first, ..] => format!(
            "embedding {} files, starting with {}",
            files.len(),
            first.file.relative_path.display()
        ),
    }
}

fn validate_context(context: &IndexingContext<'_>) -> Result<(), EngineError> {
    if context.storage.is_read_only() {
        return Err(EngineError::invalid_input(
            "indexing requires writable workspace storage",
        ));
    }
    if context.workspace_index.id.trim().is_empty() {
        return Err(EngineError::invalid_input(
            "indexing requires a workspace index id",
        ));
    }
    if context.workspace_index.roots.is_empty() {
        return Err(EngineError::invalid_input(
            "indexing requires at least one workspace root",
        ));
    }
    if context.workspace_index.policy != WorkspaceIndexPolicy::Enabled {
        return Err(EngineError::invalid_input(
            "indexing requires an enabled workspace",
        ));
    }
    if context.embedding_model.info().limits.max_batch_size == 0 {
        return Err(EngineError::backend(
            "embedding",
            "model max_batch_size must be greater than zero",
        ));
    }
    if let Some(schema) = &context.workspace_index.embedding {
        let model = context.embedding_model.info();
        if schema.provider != model.provider
            || schema.model != model.name
            || schema.dimension != model.dimension
            || schema.metric != metric_name(model.metric)
        {
            return Err(EngineError::invalid_input(format!(
                "workspace embedding schema does not match model {}",
                model.reference
            )));
        }
    }
    let _ = resolve_embedding_policy(
        context.embedding_concurrency,
        context.embedding_model.info(),
    )?;
    Ok(())
}

const fn metric_name(metric: crate::models::EmbeddingMetric) -> &'static str {
    match metric {
        crate::models::EmbeddingMetric::Cosine => "cosine",
        crate::models::EmbeddingMetric::DotProduct => "dot",
        crate::models::EmbeddingMetric::Euclidean => "euclidean",
    }
}

fn host_roots(roots: &[RootPath]) -> Vec<RootSpec> {
    roots
        .iter()
        .map(|root| RootSpec {
            path: root.path.clone(),
            recursive: root.recursive,
            discovery: HostDiscoveryOptions {
                include_paths: root.discovery.include_paths.clone(),
                exclude_paths: root.discovery.exclude_paths.clone(),
                globs: root.discovery.globs.clone(),
                insensitive_globs: root.discovery.insensitive_globs.clone(),
                file_types: root.discovery.file_types.clone(),
                excluded_file_types: root.discovery.excluded_file_types.clone(),
                hidden: root.discovery.hidden,
                no_ignore: root.discovery.no_ignore,
                ignore_files: root.discovery.ignore_files.clone(),
                max_depth: root.discovery.max_depth,
                max_file_size_bytes: root.discovery.max_file_size_bytes,
                follow: root.discovery.follow,
            },
        })
        .collect()
}

fn known_source_files(files: &[FileInfo]) -> Vec<KnownSourceFile> {
    files
        .iter()
        .map(|file| KnownSourceFile {
            root: file.root_path.clone(),
            relative_path: file.relative_path.clone(),
            source_fingerprint: metadata_fingerprint(file.size_bytes, file.modified_epoch_ms),
        })
        .collect()
}

fn metadata_fingerprint(size_bytes: u64, modified_epoch_ms: u64) -> String {
    format!("metadata-v1:{size_bytes}:{modified_epoch_ms}")
}

fn scanned_files(
    workspace_index_id: &str,
    files: Vec<DiscoveredFile>,
) -> Result<Vec<ScannedFile>, EngineError> {
    files
        .into_iter()
        .map(|discovered| {
            let absolute_path = discovered_absolute_path(&discovered);
            let kind = discovered
                .kind_hint
                .map(file_kind)
                .ok_or_else(|| EngineError::backend("native_scanner", "file kind is missing"))?;
            let format = discovered
                .format_hint
                .clone()
                .ok_or_else(|| EngineError::backend("native_scanner", "file format is missing"))?;
            Ok(ScannedFile {
                file: FileInfo {
                    id: make_file_id(workspace_index_id, &absolute_path),
                    absolute_path,
                    relative_path: discovered.relative_path.clone(),
                    root_path: discovered.root.clone(),
                    size_bytes: discovered.size_bytes,
                    modified_epoch_ms: discovered.modified_epoch_ms.unwrap_or(0),
                    content_hash: None,
                    kind,
                    format,
                    index_status: None,
                },
                discovered,
            })
        })
        .collect()
}

fn discovered_absolute_path(file: &DiscoveredFile) -> PathBuf {
    if file.root.is_file() {
        file.root.clone()
    } else {
        file.root.join(&file.relative_path)
    }
}

const fn file_kind(kind: HostFileKind) -> FileKind {
    match kind {
        HostFileKind::Text => FileKind::Text,
        HostFileKind::Code => FileKind::Code,
        HostFileKind::Data => FileKind::Data,
        HostFileKind::Image => FileKind::Image,
    }
}

fn image_format(format: &str) -> Result<ImageFormat, EngineError> {
    match format {
        "png" => Ok(ImageFormat::Png),
        "jpeg" | "jpg" => Ok(ImageFormat::Jpeg),
        "webp" => Ok(ImageFormat::Webp),
        "gif" => Ok(ImageFormat::Gif),
        _ => Err(EngineError::invalid_input(format!(
            "unsupported image format {format}"
        ))),
    }
}

async fn read_source(
    scanner: &dyn WorkspaceScannerPort,
    control: &TaskControl,
    file: &DiscoveredFile,
) -> Result<HostSource, EngineError> {
    let mut sources = scanner
        .read_batch(
            &ReadBatchRequest {
                files: vec![file.clone()],
            },
            control,
        )
        .await
        .map_err(map_host_error)?;
    if sources.len() != 1 {
        return Err(EngineError::backend(
            "native_scanner",
            format!("read_batch returned {} sources for one file", sources.len()),
        ));
    }
    Ok(sources.remove(0))
}

fn skipped_files(snapshot: &ScanSnapshot) -> Vec<SkippedFile> {
    snapshot
        .diagnostics
        .skipped_samples
        .iter()
        .map(|skipped| SkippedFile {
            path: skipped.path.clone(),
            reason: match skipped.reason {
                zg_host_native::SkippedFileReason::Empty => SkippedFileReason::Empty,
                zg_host_native::SkippedFileReason::TooLarge => SkippedFileReason::TooLarge,
                zg_host_native::SkippedFileReason::Unsupported => SkippedFileReason::Unsupported,
                zg_host_native::SkippedFileReason::Binary => SkippedFileReason::Binary,
            },
            size_bytes: skipped.size_bytes,
            limit_bytes: skipped.limit_bytes,
        })
        .collect()
}

fn make_file_id(workspace_index_id: &str, absolute_path: &Path) -> String {
    let normalized = absolute_path.to_string_lossy().replace('\\', "/");
    sha256_text(&format!("{workspace_index_id}\0{normalized}"))
}

fn sha256_text(value: &str) -> String {
    hex_digest(Sha256::digest(value.as_bytes()))
}

fn sha256_bytes(value: &[u8]) -> String {
    hex_digest(Sha256::digest(value))
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = digest.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[derive(Debug)]
enum ChangeScope {
    All,
    Paths(Vec<PathBuf>),
}

impl ChangeScope {
    fn from_changes(roots: &[RootPath], changes: &[WorkspaceChange]) -> Self {
        if changes.is_empty()
            || changes
                .iter()
                .any(|change| matches!(change, WorkspaceChange::Rescan))
        {
            return Self::All;
        }
        let mut paths = Vec::new();
        for change in changes {
            let path = match change {
                WorkspaceChange::Upsert(path)
                | WorkspaceChange::Delete(path)
                | WorkspaceChange::RescanDirectory(path)
                | WorkspaceChange::DeletePrefix(path) => path,
                WorkspaceChange::Rescan => continue,
            };
            if path.is_absolute() {
                paths.push(path.clone());
            } else {
                paths.extend(roots.iter().map(|root| root.path.join(path)));
            }
        }
        paths.sort();
        paths.dedup();
        Self::Paths(paths)
    }

    fn contains(&self, path: &Path) -> bool {
        match self {
            Self::All => true,
            Self::Paths(paths) => paths
                .iter()
                .any(|scope| path == scope || path.starts_with(scope)),
        }
    }

    fn filter_stored(&self, files: &[FileInfo]) -> Vec<FileInfo> {
        files
            .iter()
            .filter(|file| self.contains(&file.absolute_path))
            .cloned()
            .collect()
    }

    fn filter_scanned(&self, files: Vec<ScannedFile>) -> Vec<ScannedFile> {
        files
            .into_iter()
            .filter(|file| self.contains(&file.file.absolute_path))
            .collect()
    }
}

fn task_control(signal: Option<CancellationToken>) -> TaskControl {
    TaskControl::new(signal.unwrap_or_default())
}

fn throw_if_cancelled(signal: Option<&CancellationToken>) -> Result<(), EngineError> {
    if signal.is_some_and(CancellationToken::is_cancelled) {
        Err(EngineError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_host_error(error: HostError) -> EngineError {
    match error {
        HostError::InvalidInput { message } => EngineError::invalid_input(message),
        HostError::BackendFailure { backend, message } => EngineError::backend(backend, message),
        HostError::Cancelled => EngineError::Cancelled,
        HostError::DeadlineExceeded => EngineError::DeadlineExceeded,
        HostError::Closed => EngineError::Closed,
        HostError::Internal { message } => EngineError::Internal { message },
    }
}

fn build_index_result(
    context: &IndexingContext<'_>,
    passes: &[IndexPassResult],
    duration: Duration,
    timings: TimingCollector,
) -> IndexResult {
    let first = &passes[0];
    let final_pass = passes.last().expect("an index pass is always present");
    IndexResult {
        generation: context
            .workspace_index
            .generation
            .unwrap_or(0)
            .saturating_add(1),
        files_scanned: final_pass.files_scanned,
        files_added: passes.iter().map(|pass| pass.diff.added).sum(),
        files_modified: passes.iter().map(|pass| pass.diff.modified).sum(),
        files_pending: passes.iter().map(|pass| pass.diff.pending).sum(),
        files_deleted: passes.iter().map(|pass| pass.diff.deleted.len()).sum(),
        files_unchanged: first.diff.unchanged,
        files_failed: final_pass.stats.files_failed,
        entities_created: passes.iter().map(|pass| pass.stats.entities_created).sum(),
        duration_micros: duration.as_micros().try_into().unwrap_or(u64::MAX),
        timings: timings.entries,
        skipped: final_pass.skipped.clone(),
    }
}

#[derive(Default)]
struct TimingCollector {
    entries: Vec<TimingEntry>,
}

impl TimingCollector {
    fn record(&mut self, name: &str, duration: Duration, count: usize) {
        let duration_micros = duration.as_micros().try_into().unwrap_or(u64::MAX);
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.name == name) {
            entry.duration_micros = entry.duration_micros.saturating_add(duration_micros);
            entry.count = Some(entry.count.unwrap_or(0).saturating_add(count as u64));
        } else {
            self.entries.push(TimingEntry {
                name: name.to_owned(),
                duration_micros,
                count: Some(count.try_into().unwrap_or(u64::MAX)),
            });
        }
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::tempdir;
    use tokio::time::sleep;
    use zg_host_native::NativeScanner;

    use crate::{
        api::{
            index::{options::DiscoveryOptions, progress::IndexProgressPhase},
            info::result::WorkspaceIndexEmbedding,
        },
        models::{EmbeddingMetric, EmbeddingModelLimits},
        storage::spi::{
            FileIndexStatus, ListEntitiesOptions, StorageResult, StorageSearchFilter,
            StorageSearchHit, StoredEntity,
        },
    };

    use super::*;

    #[derive(Default)]
    struct MemoryStorage {
        files: Mutex<Vec<FileInfo>>,
        finalized: AtomicUsize,
    }

    #[async_trait]
    impl WorkspaceIndexStorage for MemoryStorage {
        fn is_read_only(&self) -> bool {
            false
        }

        fn get_file_by_path(&self, absolute_path: &Path) -> StorageResult<Option<FileInfo>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .find(|file| file.absolute_path == absolute_path)
                .cloned())
        }

        fn list_files_by_path_prefix(&self, absolute_path: &Path) -> StorageResult<Vec<FileInfo>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .filter(|file| file.absolute_path.starts_with(absolute_path))
                .cloned()
                .collect())
        }

        fn list_files_by_path_prefixes(
            &self,
            absolute_paths: &[PathBuf],
        ) -> StorageResult<Vec<FileInfo>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .filter(|file| {
                    absolute_paths
                        .iter()
                        .any(|path| file.absolute_path.starts_with(path))
                })
                .cloned()
                .collect())
        }

        fn list_files(&self) -> StorageResult<Vec<FileInfo>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone())
        }

        fn list_entities_by_file(
            &self,
            _file_id: &str,
            _options: ListEntitiesOptions,
        ) -> StorageResult<Vec<StoredEntity>> {
            Ok(Vec::new())
        }

        fn get_entity(&self, _entity_id: &str) -> StorageResult<Option<StoredEntity>> {
            Ok(None)
        }

        fn search_fts(
            &self,
            _query: &str,
            _limit: usize,
            _filter: Option<&StorageSearchFilter>,
        ) -> StorageResult<Vec<StorageSearchHit>> {
            Ok(Vec::new())
        }

        fn search_vector(
            &self,
            _vector: &[f32],
            _limit: usize,
            _filter: Option<&StorageSearchFilter>,
        ) -> StorageResult<Vec<StorageSearchHit>> {
            Ok(Vec::new())
        }

        fn replace_file(
            &self,
            file: &FileInfo,
            entries: &[IndexedFragment],
            diagnostics: Option<&FileIndexDiagnostics>,
        ) -> StorageResult<()> {
            let mut stored = file.clone();
            stored.index_status = Some(FileIndexStatus {
                indexed_epoch_ms: Some(1),
                entity_count: entries.len(),
                token_count: None,
                truncated_fragment_count: diagnostics
                    .and_then(|diagnostics| diagnostics.truncated_fragment_count),
                error: None,
            });
            let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(existing) = files.iter_mut().find(|existing| existing.id == stored.id) {
                *existing = stored;
            } else {
                files.push(stored);
            }
            Ok(())
        }

        fn mark_file_failed(&self, file: &FileInfo, error: &str) -> StorageResult<()> {
            let mut stored = file.clone();
            stored.index_status = Some(FileIndexStatus {
                indexed_epoch_ms: None,
                entity_count: 0,
                token_count: None,
                truncated_fragment_count: None,
                error: Some(error.to_owned()),
            });
            let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(existing) = files.iter_mut().find(|existing| existing.id == stored.id) {
                *existing = stored;
            } else {
                files.push(stored);
            }
            Ok(())
        }

        fn delete_file(&self, file_id: &str) -> StorageResult<()> {
            self.files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|file| file.id != file_id);
            Ok(())
        }

        async fn finalize_writes(&self) -> StorageResult<()> {
            self.finalized.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }

        fn close(&self) -> StorageResult<()> {
            Ok(())
        }
    }

    struct ConcurrentModel {
        info: EmbeddingModelInfo,
        calls: AtomicUsize,
        active: AtomicUsize,
        maximum_active: AtomicUsize,
    }

    impl ConcurrentModel {
        fn new() -> Self {
            Self {
                info: EmbeddingModelInfo {
                    reference: "local/test".to_owned(),
                    provider: "local".to_owned(),
                    name: "test".to_owned(),
                    dimension: 2,
                    metric: EmbeddingMetric::Cosine,
                    endpoint: None,
                    default_concurrency: Some(2),
                    input_kinds: vec![EmbeddingInputKind::Text],
                    limits: EmbeddingModelLimits {
                        max_batch_size: 1,
                        max_input_tokens: Some(64),
                        max_image_bytes: None,
                    },
                },
                calls: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                maximum_active: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl IndexEmbeddingRuntime for ConcurrentModel {
        fn info(&self) -> &EmbeddingModelInfo {
            &self.info
        }

        async fn embed(
            &self,
            contents: &[Content],
            _options: EmbeddingOptions,
            _progress: Option<IndexProgressReporter>,
        ) -> Result<EmbeddingResult, ModelError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
            self.maximum_active.fetch_max(active, Ordering::AcqRel);
            sleep(Duration::from_millis(10)).await;
            self.active.fetch_sub(1, Ordering::AcqRel);
            Ok(EmbeddingResult {
                vectors: contents.iter().map(|_| vec![1.0, 0.0]).collect(),
                truncated: Vec::new(),
            })
        }
    }

    fn workspace(root: &Path) -> WorkspaceIndexInfo {
        WorkspaceIndexInfo {
            id: "workspace-id".to_owned(),
            name: "fixture".to_owned(),
            path: root.join(".zvec-grep"),
            roots: vec![RootPath {
                path: root.to_path_buf(),
                recursive: true,
                discovery: DiscoveryOptions {
                    no_ignore: true,
                    ..DiscoveryOptions::default()
                },
            }],
            policy: WorkspaceIndexPolicy::Enabled,
            embedding: Some(WorkspaceIndexEmbedding {
                provider: "local".to_owned(),
                model: "test".to_owned(),
                dimension: 2,
                metric: "cosine".to_owned(),
            }),
            index_version: Some(1),
            generation: None,
            created_epoch_ms: 1,
            updated_epoch_ms: 1,
        }
    }

    #[tokio::test]
    async fn indexes_incrementally_reuses_unchanged_files_and_honors_concurrency() {
        let directory = tempdir().expect("temporary directory");
        for index in 0..4 {
            std::fs::write(
                directory.path().join(format!("file-{index}.txt")),
                format!("content {index}"),
            )
            .expect("fixture file");
        }
        let workspace = workspace(directory.path());
        let scanner = NativeScanner::default();
        let storage = MemoryStorage::default();
        let model = ConcurrentModel::new();
        let progress = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&progress);
        let reporter = IndexProgressReporter::new(move |value| {
            captured
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(value);
        });
        let context = IndexingContext {
            workspace_index: &workspace,
            storage: &storage,
            scanner: &scanner,
            embedding_model: &model,
            embedding_concurrency: Some(2),
            on_progress: Some(reporter),
            signal: None,
            changes: &[],
        };

        let first = index_workspace(&context).await.expect("initial index");
        assert_eq!(first.files_added, 4);
        assert_eq!(first.entities_created, 4);
        assert_eq!(storage.list_files().expect("stored files").len(), 4);
        assert_eq!(model.maximum_active.load(Ordering::Acquire), 2);
        assert!(
            progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|item| item.phase == IndexProgressPhase::Done)
        );

        let calls = model.calls.load(Ordering::Acquire);
        let second = index_workspace(&context).await.expect("unchanged index");
        assert_eq!(second.files_unchanged, 4);
        assert_eq!(second.entities_created, 0);
        assert_eq!(model.calls.load(Ordering::Acquire), calls);

        std::fs::write(directory.path().join("file-0.txt"), "changed and longer")
            .expect("modified file");
        let third = index_workspace(&context).await.expect("modified index");
        assert_eq!(third.files_modified, 1);
        assert_eq!(third.files_unchanged, 3);

        std::fs::remove_file(directory.path().join("file-3.txt")).expect("deleted file");
        let fourth = index_workspace(&context).await.expect("deleted index");
        assert_eq!(fourth.files_deleted, 1);
        assert_eq!(storage.list_files().expect("stored files").len(), 3);
        assert_eq!(storage.finalized.load(Ordering::Acquire), 4);
    }

    #[tokio::test]
    async fn changed_paths_limit_diff_and_storage_mutation_scope() {
        let directory = tempdir().expect("temporary directory");
        let first_path = directory.path().join("first.txt");
        let second_path = directory.path().join("second.txt");
        std::fs::write(&first_path, "first").expect("first fixture");
        std::fs::write(&second_path, "second").expect("second fixture");
        let workspace = workspace(directory.path());
        let scanner = NativeScanner::default();
        let storage = MemoryStorage::default();
        let model = ConcurrentModel::new();
        index_workspace(&IndexingContext {
            workspace_index: &workspace,
            storage: &storage,
            scanner: &scanner,
            embedding_model: &model,
            embedding_concurrency: Some(2),
            on_progress: None,
            signal: None,
            changes: &[],
        })
        .await
        .expect("initial index");

        std::fs::write(&first_path, "first changed").expect("first modified");
        std::fs::write(&second_path, "second changed").expect("second modified");
        let changes = [WorkspaceChange::Upsert(first_path.clone())];
        let result = index_workspace(&IndexingContext {
            workspace_index: &workspace,
            storage: &storage,
            scanner: &scanner,
            embedding_model: &model,
            embedding_concurrency: Some(2),
            on_progress: None,
            signal: None,
            changes: &changes,
        })
        .await
        .expect("narrow index");

        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.files_modified, 1);
        let stored = storage.list_files().expect("stored files");
        let untouched = stored
            .iter()
            .find(|file| file.absolute_path == second_path)
            .expect("second stored file");
        let changed_hash = sha256_bytes(b"second changed");
        assert_ne!(
            untouched.content_hash.as_deref(),
            Some(changed_hash.as_str())
        );
    }

    #[test]
    fn keeps_host_errors_outside_the_engine_api_boundary() {
        let error = map_host_error(HostError::Cancelled);
        assert_eq!(error.code(), crate::ErrorCode::Cancelled);
        let invalid = map_host_error(HostError::invalid_input("bad root"));
        assert_eq!(invalid.code(), crate::ErrorCode::InvalidInput);
    }
}
