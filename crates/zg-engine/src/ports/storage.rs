use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;

use crate::{Content, ContentRange, CoreError, EntityMetadata, RunControl};

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
pub struct IndexSnapshot {
    pub root: PathBuf,
    pub generation: u64,
    pub index_version: u32,
    pub model_fingerprint: Option<String>,
    pub entity_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum IndexMutation {
    Upsert(Box<StoredEntity>),
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

/// Recall and transactional generation storage seam.
#[async_trait]
pub trait IndexStoragePort: Send + Sync {
    async fn inspect(&self, root: &std::path::Path) -> Result<Option<IndexSnapshot>, CoreError>;

    async fn recall_batch(
        &self,
        request: &RecallRequest,
        control: &RunControl,
    ) -> Result<Vec<RecallHit>, CoreError>;

    async fn begin_write(
        &self,
        root: &std::path::Path,
        mode: WriteMode,
        control: &RunControl,
    ) -> Result<Arc<dyn IndexWritePort>, CoreError>;
}
