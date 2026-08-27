use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{DiscoveryOptions, EmbeddingModelSpec, RootSpec, TimingEntry};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct IndexRequest {
    pub roots: Vec<RootSpec>,
    pub rebuild: bool,
    pub reset_paths: bool,
    /// Normalized watcher changes for a narrow incremental index operation.
    /// An empty list means normal discovery rather than "no work".
    pub changes: Vec<WorkspaceChange>,
    pub discovery: DiscoveryOptions,
    pub embedding: Option<EmbeddingModelSpec>,
    pub embedding_concurrency: Option<usize>,
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
pub struct WorkspaceChangeBatch {
    pub changes: Vec<WorkspaceChange>,
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
