use std::path::PathBuf;

use async_trait::async_trait;

use crate::{Content, ContentRange, CoreError, EntityMetadata, FileKind, RunControl};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractInput {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub kind_hint: Option<FileKind>,
    pub format_hint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedDocument {
    pub path: PathBuf,
    pub kind: FileKind,
    pub format: String,
    pub entities: Vec<ExtractedEntity>,
    pub warnings: Vec<ExtractionWarning>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedEntity {
    pub stable_id: String,
    pub range: ContentRange,
    pub content: Content,
    pub metadata: Option<EntityMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractionWarning {
    pub code: String,
    pub message: String,
}

/// Batch-oriented document extraction seam.
#[async_trait]
pub trait ExtractionPort: Send + Sync {
    async fn extract_batch(
        &self,
        inputs: Vec<ExtractInput>,
        control: &RunControl,
    ) -> Result<Vec<ExtractedDocument>, CoreError>;
}
