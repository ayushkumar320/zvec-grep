use std::sync::Arc;

use async_trait::async_trait;

use crate::{Content, CoreError, EmbeddingModelSpec, RunControl};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmbeddingMetric {
    Cosine,
    DotProduct,
    Euclidean,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmbeddingInputKind {
    Text,
    Image,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddingModelInfo {
    pub reference: String,
    pub revision: String,
    pub dimension: usize,
    pub metric: EmbeddingMetric,
    pub input_kinds: Vec<EmbeddingInputKind>,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddingInput {
    pub id: String,
    pub content: Content,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddingVector {
    pub id: String,
    pub values: Vec<f32>,
    pub truncated: bool,
}

/// A loaded, reusable model session.
#[async_trait]
pub trait EmbeddingSessionPort: Send + Sync {
    fn info(&self) -> &EmbeddingModelInfo;

    async fn embed_batch(
        &self,
        inputs: Vec<EmbeddingInput>,
        control: &RunControl,
    ) -> Result<Vec<EmbeddingVector>, CoreError>;

    async fn close(&self) -> Result<(), CoreError>;
}

/// Materializes and loads model sessions without exposing runtime-specific types.
#[async_trait]
pub trait EmbeddingFactoryPort: Send + Sync {
    async fn load(
        &self,
        model: &EmbeddingModelSpec,
        control: &RunControl,
    ) -> Result<Arc<dyn EmbeddingSessionPort>, CoreError>;
}
