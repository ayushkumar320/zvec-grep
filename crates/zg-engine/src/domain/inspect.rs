use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{EmbeddingModelSpec, IndexPolicy, RootSpec};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectRequest {
    pub include_status: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectReply {
    pub root: PathBuf,
    pub indexed: bool,
    pub index_policy: IndexPolicy,
    pub home: PathBuf,
    pub index_path: PathBuf,
    pub source: InspectSource,
    pub workspace_index: Option<WorkspaceIndexInfo>,
    pub status: Option<WorkspaceIndexStatus>,
    pub suggestion: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectSource {
    Index,
    Unindexed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceIndexInfo {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub roots: Vec<RootSpec>,
    pub policy: IndexPolicy,
    pub embedding: Option<EmbeddingModelSpec>,
    pub index_version: Option<u32>,
    pub generation: Option<u64>,
    pub created_epoch_ms: u64,
    pub updated_epoch_ms: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceIndexStatus {
    pub files_scanned: usize,
    pub files_stored: usize,
    pub files_indexed: usize,
    pub entities_indexed: usize,
    pub fragments_truncated: usize,
    pub files_pending: usize,
    pub files_failed: usize,
    pub files_added: usize,
    pub files_modified: usize,
    pub files_deleted: usize,
    pub files_unchanged: usize,
}
