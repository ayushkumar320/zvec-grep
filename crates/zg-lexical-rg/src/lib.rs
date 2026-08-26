//! Production lexical adapter backed by the official ripgrep executable.

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use async_trait::async_trait;
use serde::Deserialize;
use tokio::{process::Command, sync::Semaphore};
use tracing::debug;
use zg_engine::{
    CoreError, LexicalCoverage, LexicalDiagnostics, LexicalMatch, LexicalSearchPort,
    LexicalSearchReply, LexicalSearchRequest, RunControl, TextRange,
};

const HARD_IGNORED_DIRECTORIES: [&str; 2] = [".git", ".zvec-grep"];

#[derive(Clone, Debug)]
pub struct RipgrepAdapter {
    executable: PathBuf,
    process_slots: std::sync::Arc<Semaphore>,
}

impl RipgrepAdapter {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            process_slots: std::sync::Arc::new(Semaphore::new(1)),
        }
    }

    #[must_use]
    pub fn with_max_processes(mut self, max_processes: usize) -> Self {
        self.process_slots = std::sync::Arc::new(Semaphore::new(max_processes.max(1)));
        self
    }
}

impl Default for RipgrepAdapter {
    fn default() -> Self {
        Self::new("rg")
    }
}

#[async_trait]
impl LexicalSearchPort for RipgrepAdapter {
    async fn search(
        &self,
        root: &Path,
        request: &LexicalSearchRequest,
        control: &RunControl,
    ) -> Result<LexicalSearchReply, CoreError> {
        if request.patterns.is_empty() && request.pattern_files.is_empty() {
            return Err(CoreError::invalid_input(
                "lexical search requires a pattern or pattern file",
            ));
        }

        let _process_slot = tokio::select! {
            () = control.cancellation.cancelled() => return Err(CoreError::Cancelled),
            permit = self.process_slots.acquire() => permit.map_err(|_| CoreError::ShuttingDown)?,
        };

        let checked_paths = check_paths(root, &request.paths);
        let args = build_args(request, &checked_paths.existing);
        if !request.paths.is_empty() && checked_paths.existing.is_empty() {
            return Ok(empty_reply(
                root,
                &self.executable,
                &args,
                request,
                &checked_paths,
            ));
        }

        debug!(command = %self.executable.display(), ?args, "running ripgrep");
        let mut command = Command::new(&self.executable);
        command
            .args(&args)
            .current_dir(root)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = tokio::select! {
            () = control.cancellation.cancelled() => return Err(CoreError::Cancelled),
            result = command.output() => result.map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    CoreError::CapabilityUnavailable {
                        capability: format!("ripgrep executable {}", self.executable.display()),
                    }
                } else {
                    CoreError::backend("ripgrep", error.to_string())
                }
            })?,
        };

        if !matches!(output.status.code(), Some(0 | 1)) {
            return Err(CoreError::backend(
                "ripgrep",
                format!(
                    "exit status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }

        let mut matches = parse_output(root, &output.stdout);
        expand_context(&mut matches, request);
        matches.retain(|item| matches_modified_time(&item.absolute_path, request));
        matches.sort_by(|left, right| {
            left.relative_path
                .cmp(&right.relative_path)
                .then(left.range.start_line.cmp(&right.range.start_line))
                .then(left.range.start_offset.cmp(&right.range.start_offset))
        });

        let truncated = request.limit.is_some_and(|limit| matches.len() > limit);
        if let Some(limit) = request.limit {
            matches.truncate(limit);
        }
        for (index, item) in matches.iter_mut().enumerate() {
            item.rank = index + 1;
        }

        Ok(LexicalSearchReply {
            root: root.to_path_buf(),
            coverage: if truncated {
                LexicalCoverage::Truncated
            } else {
                LexicalCoverage::Exhaustive
            },
            matches,
            diagnostics: diagnostics(&self.executable, &args, request, &checked_paths, truncated),
        })
    }
}

#[derive(Debug)]
struct CheckedPaths {
    existing: Vec<PathBuf>,
    missing: Vec<PathBuf>,
}

fn check_paths(root: &Path, paths: &[PathBuf]) -> CheckedPaths {
    let (existing, missing) = paths
        .iter()
        .cloned()
        .partition(|path| resolve_path(root, path).exists());
    CheckedPaths { existing, missing }
}

fn resolve_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn build_args(request: &LexicalSearchRequest, paths: &[PathBuf]) -> Vec<OsString> {
    let mut args = [
        "--json",
        "--line-number",
        "--column",
        "--with-filename",
        "--color",
        "never",
    ]
    .map(OsString::from)
    .to_vec();

    push_switch(&mut args, request.options.fixed_strings, "--fixed-strings");
    push_switch(&mut args, request.options.ignore_case, "--ignore-case");
    push_switch(&mut args, request.options.word_regexp, "--word-regexp");
    push_switch(&mut args, request.options.hidden, "--hidden");
    push_switch(&mut args, request.options.no_ignore, "--no-ignore");
    push_switch(&mut args, request.options.follow, "--follow");
    push_value(
        &mut args,
        "--max-depth",
        request.options.max_depth.map(|value| value.to_string()),
    );
    push_value(
        &mut args,
        "--max-filesize",
        request
            .options
            .max_file_size_bytes
            .map(|value| value.to_string()),
    );

    for path in &request.options.ignore_files {
        args.extend([OsString::from("--ignore-file"), path.as_os_str().to_owned()]);
    }
    for glob in &request.options.globs {
        args.extend([OsString::from("--glob"), OsString::from(glob)]);
    }
    for file_type in &request.options.file_types {
        args.extend([OsString::from("--type"), OsString::from(file_type)]);
    }
    for file_type in &request.options.excluded_file_types {
        args.extend([OsString::from("--type-not"), OsString::from(file_type)]);
    }
    for directory in HARD_IGNORED_DIRECTORIES {
        args.extend([
            OsString::from("--glob"),
            OsString::from(format!("!**/{directory}/**")),
        ]);
    }
    for pattern in &request.patterns {
        args.extend([OsString::from("--regexp"), OsString::from(pattern)]);
    }
    for pattern_file in &request.pattern_files {
        args.extend([
            OsString::from("--file"),
            pattern_file.as_os_str().to_owned(),
        ]);
    }
    args.push(OsString::from("--"));
    if paths.is_empty() {
        args.push(OsString::from("."));
    } else {
        args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
    }
    args
}

fn push_switch(args: &mut Vec<OsString>, enabled: bool, name: &str) {
    if enabled {
        args.push(OsString::from(name));
    }
}

fn push_value(args: &mut Vec<OsString>, name: &str, value: Option<String>) {
    if let Some(value) = value {
        args.extend([OsString::from(name), OsString::from(value)]);
    }
}

fn empty_reply(
    root: &Path,
    executable: &Path,
    args: &[OsString],
    request: &LexicalSearchRequest,
    checked_paths: &CheckedPaths,
) -> LexicalSearchReply {
    LexicalSearchReply {
        root: root.to_path_buf(),
        coverage: LexicalCoverage::Exhaustive,
        matches: Vec::new(),
        diagnostics: diagnostics(executable, args, request, checked_paths, false),
    }
}

fn diagnostics(
    executable: &Path,
    args: &[OsString],
    request: &LexicalSearchRequest,
    checked_paths: &CheckedPaths,
    truncated: bool,
) -> LexicalDiagnostics {
    LexicalDiagnostics {
        backend: "rg".to_owned(),
        command: executable.to_path_buf(),
        args: args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        ignored_directories: HARD_IGNORED_DIRECTORIES
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        missing_paths: checked_paths.missing.clone(),
        searched_paths: checked_paths.existing.clone(),
        limit: request.limit,
        truncated,
    }
}

fn matches_modified_time(path: &Path, request: &LexicalSearchRequest) -> bool {
    let Some(after) = request.options.modified_after_epoch_ms else {
        return request
            .options
            .modified_before_epoch_ms
            .is_none_or(|before| modified_epoch_ms(path).is_some_and(|value| value <= before));
    };
    let Some(modified) = modified_epoch_ms(path) else {
        return false;
    };
    modified >= after
        && request
            .options
            .modified_before_epoch_ms
            .is_none_or(|before| modified <= before)
}

fn expand_context(matches: &mut [LexicalMatch], request: &LexicalSearchRequest) {
    let before = request.options.before_context;
    let after = request.options.after_context;
    if before == 0 && after == 0 {
        return;
    }

    let mut cache: HashMap<PathBuf, Option<Vec<String>>> = HashMap::new();
    for item in matches {
        let lines = cache.entry(item.absolute_path.clone()).or_insert_with(|| {
            std::fs::read_to_string(&item.absolute_path)
                .ok()
                .map(|content| content.lines().map(str::to_owned).collect())
        });
        let Some(lines) = lines else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }

        let excerpt = item.range.clone();
        let start_line = excerpt.start_line.saturating_sub(before).max(1);
        let end_line = excerpt.end_line.saturating_add(after).min(lines.len());
        let content = lines[start_line - 1..end_line].join("\n");
        let end_offset = lines[end_line - 1].encode_utf16().count();
        item.excerpt_range = Some(excerpt);
        item.range = TextRange {
            start_line,
            end_line,
            start_offset: 0,
            end_offset,
        };
        item.content = content;
    }
}

fn modified_epoch_ms(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

#[derive(Debug, Deserialize)]
struct RipgrepEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Option<RipgrepMatchData>,
}

#[derive(Debug, Deserialize)]
struct RipgrepMatchData {
    path: TextValue,
    lines: TextValue,
    line_number: usize,
    #[serde(default)]
    submatches: Vec<RipgrepSubmatch>,
}

#[derive(Debug, Deserialize)]
struct TextValue {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RipgrepSubmatch {
    start: usize,
    end: usize,
}

fn parse_output(root: &Path, stdout: &[u8]) -> Vec<LexicalMatch> {
    stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<RipgrepEvent>(line).ok())
        .filter_map(|event| parse_event(root, event))
        .collect()
}

fn parse_event(root: &Path, event: RipgrepEvent) -> Option<LexicalMatch> {
    if event.kind != "match" {
        return None;
    }
    let data = event.data?;
    let path = PathBuf::from(data.path.text?);
    let absolute_path = resolve_path(root, &path);
    let relative_path = absolute_path
        .strip_prefix(root)
        .map_or_else(|_| absolute_path.clone(), Path::to_path_buf);
    let content = data.lines.text?.trim_end_matches(['\r', '\n']).to_owned();
    let first = data.submatches.first();
    let start = text_position_at_byte_offset(&content, first.map_or(0, |item| item.start));
    let end = text_position_at_byte_offset(&content, first.map_or(content.len(), |item| item.end));

    Some(LexicalMatch {
        rank: 0,
        absolute_path,
        relative_path,
        range: TextRange {
            start_line: data.line_number + start.0,
            end_line: data.line_number + end.0,
            start_offset: start.1,
            end_offset: end.1,
        },
        excerpt_range: None,
        content,
    })
}

fn text_position_at_byte_offset(value: &str, byte_offset: usize) -> (usize, usize) {
    let end = byte_offset.min(value.len());
    let prefix = String::from_utf8_lossy(&value.as_bytes()[..end]);
    let line_offset = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let last = prefix.rsplit('\n').next().unwrap_or_default();
    (
        line_offset,
        last.trim_end_matches('\r').encode_utf16().count(),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_output;

    #[test]
    fn parses_match_events_and_uses_utf16_columns() {
        let line = r#"{"type":"match","data":{"path":{"text":"src/a.rs"},"lines":{"text":"let x = \"你好\";\n"},"line_number":7,"absolute_offset":0,"submatches":[{"match":{"text":"你好"},"start":9,"end":15}]}}"#;
        let matches = parse_output(std::path::Path::new("/workspace"), line.as_bytes());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].range.start_line, 7);
        assert_eq!(matches[0].range.start_offset, 9);
        assert_eq!(matches[0].range.end_offset, 11);
    }
}
