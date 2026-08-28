//! Index lifecycle requests, progress and replies.

use std::{fmt, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};

use super::{DiscoveryOptions, EmbeddingModelSpec, RootSpec, TimingEntry};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct IndexRequest {
    /// Workspace whose index is being updated. `None` uses the working directory.
    pub root: Option<PathBuf>,
    pub roots: Vec<RootSpec>,
    pub rebuild: bool,
    pub reset_paths: bool,
    /// Normalized watcher changes for a narrow incremental index operation.
    /// An empty list means normal discovery rather than "no work".
    pub changes: Vec<WorkspaceChange>,
    pub discovery: DiscoveryOptions,
    pub embedding: Option<EmbeddingModelSpec>,
    /// Maximum embedding batch tasks for this index operation.
    /// The model default is used when omitted.
    pub embedding_concurrency: Option<usize>,
    /// Wait for the submitted index job instead of returning an accepted job.
    pub wait: bool,
    /// Include bounded skipped-file diagnostics in a completed response.
    pub debug: bool,
    /// Receives in-process indexing and model download progress.
    ///
    /// Reporters are runtime-only and are deliberately omitted from serialized
    /// daemon and transport requests.
    #[serde(skip)]
    pub on_progress: Option<IndexProgressReporter>,
}

/// Thread-safe callback used by the in-process [`crate::ZvecGrep::index`] API.
#[derive(Clone)]
pub struct IndexProgressReporter(Arc<dyn Fn(IndexProgress) + Send + Sync + 'static>);

impl IndexProgressReporter {
    #[must_use]
    pub fn new(reporter: impl Fn(IndexProgress) + Send + Sync + 'static) -> Self {
        Self(Arc::new(reporter))
    }

    pub(crate) fn report(&self, progress: IndexProgress) {
        (self.0)(progress);
    }
}

impl fmt::Debug for IndexProgressReporter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IndexProgressReporter(..)")
    }
}

impl PartialEq for IndexProgressReporter {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for IndexProgressReporter {}

/// Current phase of an index operation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexProgressPhase {
    Scanning,
    Indexing,
    Done,
}

/// Current lifecycle stage of the embedding model used by indexing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexEmbeddingStage {
    Preparing,
    Downloading,
    Ready,
    Warning,
}

/// Model runtime and download progress nested in [`IndexProgress`].
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexEmbeddingProgress {
    pub concurrency: Option<usize>,
    pub max_concurrency: Option<usize>,
    pub retryable_failures: Option<usize>,
    pub stage: Option<IndexEmbeddingStage>,
    pub model: Option<String>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub message: Option<String>,
}

/// Progress emitted by an index operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexProgress {
    pub phase: IndexProgressPhase,
    pub files_total: Option<usize>,
    pub files_indexed: Option<usize>,
    pub files_failed: Option<usize>,
    pub detail: Option<String>,
    pub embedding: Option<IndexEmbeddingProgress>,
}

/// A normalized filesystem change relative to its [`RootSpec`].
///
/// These variants deliberately retain deletion and directory scope. Flattening
/// them into paths would make an incremental index unable to distinguish an
/// upsert from a delete or a watcher overflow from a file event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "path")]
pub enum WorkspaceChange {
    Upsert(PathBuf),
    Delete(PathBuf),
    RescanDirectory(PathBuf),
    DeletePrefix(PathBuf),
    Rescan,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexReply {
    pub generation: u64,
    pub files_scanned: usize,
    pub files_added: usize,
    pub files_modified: usize,
    pub files_pending: usize,
    pub files_deleted: usize,
    pub files_unchanged: usize,
    pub files_failed: usize,
    pub entities_created: usize,
    pub duration_micros: u64,
    pub timings: Vec<TimingEntry>,
    pub skipped: Vec<SkippedFile>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub reason: SkippedFileReason,
    pub size_bytes: Option<u64>,
    pub limit_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedFileReason {
    Empty,
    TooLarge,
    Unsupported,
    Binary,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeIndexAction {
    Drop,
    Disable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeIndexRequest {
    /// Workspace whose index is being changed. `None` uses the working directory.
    pub root: Option<PathBuf>,
    pub action: ChangeIndexAction,
    pub force: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeIndexReply {
    pub changed: bool,
    pub index_path: PathBuf,
    pub policy: IndexPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexPolicy {
    Enabled,
    Disabled,
    Undecided,
}

#[cfg(test)]
mod tests {
    use super::{IndexProgressReporter, IndexRequest};

    #[test]
    fn index_progress_reporter_is_runtime_only() {
        let request = IndexRequest {
            root: Some("/workspace".into()),
            on_progress: Some(IndexProgressReporter::new(|_| {})),
            ..IndexRequest::default()
        };

        let encoded = serde_json::to_string(&request).expect("index request should serialize");
        assert!(!encoded.contains("on_progress"));
        let decoded: IndexRequest =
            serde_json::from_str(&encoded).expect("index request should deserialize");
        assert!(decoded.on_progress.is_none());
        assert_eq!(decoded.root, request.root);
    }
}
