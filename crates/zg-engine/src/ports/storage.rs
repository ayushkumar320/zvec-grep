use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;

use crate::{Content, ContentRange, CoreError, EmbeddingMetric, EntityMetadata, RunControl};

#[derive(Clone, Debug, PartialEq)]
pub struct RecallRequest {
    pub root: PathBuf,
    pub generation: Option<u64>,
    pub routes: Vec<RecallRoute>,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecallRoute {
    pub id: String,
    pub query: RecallQuery,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecallQuery {
    Fts(String),
    Vector(Vec<f32>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecallHit {
    pub entity_id: String,
    pub file_path: PathBuf,
    pub range: ContentRange,
    pub content: Content,
    pub metadata: Option<EntityMetadata>,
    pub route_id: String,
    pub rank: usize,
    pub score: f64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedModelInfo {
    pub fingerprint: String,
    pub dimension: usize,
    pub metric: EmbeddingMetric,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexSnapshot {
    pub root: PathBuf,
    pub generation: u64,
    pub index_version: u32,
    pub model: Option<IndexedModelInfo>,
    pub file_count: usize,
    pub entity_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedFileState {
    pub relative_path: PathBuf,
    pub source_fingerprint: String,
    pub size_bytes: u64,
    pub modified_epoch_ms: Option<u64>,
    pub entity_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexedFile {
    pub relative_path: PathBuf,
    pub source_fingerprint: String,
    pub size_bytes: u64,
    pub modified_epoch_ms: Option<u64>,
    pub entities: Vec<StoredEntity>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum IndexMutation {
    ReplaceFile(Box<IndexedFile>),
    DeleteFile(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredEntity {
    pub entity_id: String,
    pub file_path: PathBuf,
    pub range: ContentRange,
    pub content: Content,
    pub metadata: Option<EntityMetadata>,
    pub vector: Option<Vec<f32>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteMode {
    Incremental,
    Rebuild,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeginWriteRequest {
    pub root: PathBuf,
    pub mode: WriteMode,
    pub model: Option<IndexedModelInfo>,
}

/// One isolated generation write. `finalize` is the only publication point.
#[async_trait]
pub trait IndexWritePort: Send + Sync {
    async fn apply_mutations(
        &self,
        mutations: Vec<IndexMutation>,
        control: &RunControl,
    ) -> Result<(), CoreError>;

    async fn finalize(&self, control: &RunControl) -> Result<IndexSnapshot, CoreError>;

    async fn abort(&self) -> Result<(), CoreError>;
}

/// Recall, file-state inspection and transactional generation storage seam.
#[async_trait]
pub trait IndexStoragePort: Send + Sync {
    async fn inspect(&self, root: &std::path::Path) -> Result<Option<IndexSnapshot>, CoreError>;

    /// Returns all file states when `paths` is empty, otherwise only matching
    /// paths. Results must be ordered by relative path.
    async fn file_states(
        &self,
        root: &std::path::Path,
        paths: &[PathBuf],
        control: &RunControl,
    ) -> Result<Vec<IndexedFileState>, CoreError>;

    async fn recall_batch(
        &self,
        request: &RecallRequest,
        control: &RunControl,
    ) -> Result<Vec<RecallHit>, CoreError>;

    async fn begin_write(
        &self,
        request: &BeginWriteRequest,
        control: &RunControl,
    ) -> Result<Arc<dyn IndexWritePort>, CoreError>;
}
