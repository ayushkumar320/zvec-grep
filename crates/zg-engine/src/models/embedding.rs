use std::{collections::HashSet, fmt, path::PathBuf, sync::Arc};

use crate::{Content, Device, EmbeddingInputKind, EmbeddingMetric};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::error::ModelError;

#[derive(Default)]
pub struct CreateEmbeddingModelOptions {
    pub api_key: Option<String>,
    pub endpoint: Option<String>,
    pub model_cache_dir: Option<PathBuf>,
    pub device: Option<Device>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EmbeddingPurpose {
    #[default]
    Document,
    Query,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmbeddingModelProgress {
    Preparing {
        model: String,
    },
    Downloading {
        model: String,
        downloaded_bytes: Option<u64>,
        total_bytes: Option<u64>,
    },
    Warning {
        model: String,
        message: String,
    },
    Ready {
        model: String,
    },
}

#[derive(Clone, Default)]
pub struct EmbeddingOptions {
    pub purpose: Option<EmbeddingPurpose>,
    pub signal: Option<CancellationToken>,
    pub on_progress: Option<Arc<dyn Fn(EmbeddingModelProgress) + Send + Sync>>,
}

impl fmt::Debug for EmbeddingOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbeddingOptions")
            .field("purpose", &self.purpose)
            .field("has_signal", &self.signal.is_some())
            .field("has_progress_callback", &self.on_progress.is_some())
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddingResult {
    pub vectors: Vec<Vec<f32>>,
    pub truncated: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::struct_field_names)]
pub struct EmbeddingModelLimits {
    pub max_batch_size: usize,
    pub max_input_tokens: Option<usize>,
    pub max_image_bytes: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddingModelInfo {
    pub reference: String,
    pub provider: String,
    pub name: String,
    pub dimension: usize,
    pub metric: EmbeddingMetric,
    pub endpoint: Option<String>,
    pub default_concurrency: Option<usize>,
    pub input_kinds: Vec<EmbeddingInputKind>,
    pub limits: EmbeddingModelLimits,
}

#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    fn info(&self) -> &EmbeddingModelInfo;

    async fn embed(
        &self,
        contents: &[Content],
        options: EmbeddingOptions,
    ) -> Result<EmbeddingResult, ModelError>;

    async fn dispose(&self) -> Result<(), ModelError>;
}

pub(crate) fn validate_contents(
    info: &EmbeddingModelInfo,
    contents: &[Content],
) -> Result<(), ModelError> {
    if contents.is_empty() {
        return Err(ModelError::coded(
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_EMPTY_INPUT",
            "Embedding requires at least one content item",
            None,
        ));
    }
    if contents.len() > info.limits.max_batch_size {
        return Err(ModelError::coded(
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_BATCH_TOO_LARGE",
            "Embedding batch size exceeds model limit",
            Some(format!(
                "model={} batchSize={} maxBatchSize={}",
                info.reference,
                contents.len(),
                info.limits.max_batch_size
            )),
        ));
    }

    for (index, content) in contents.iter().enumerate() {
        let kind = match content {
            Content::Text(_) => EmbeddingInputKind::Text,
            Content::Image(_) => EmbeddingInputKind::Image,
        };
        if !info.input_kinds.contains(&kind) {
            return Err(ModelError::coded(
                "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_UNSUPPORTED_CONTENT",
                "Embedding model does not support content kind",
                Some(format!(
                    "model={} index={index} kind={}",
                    info.reference,
                    kind_name(kind)
                )),
            ));
        }

        match content {
            Content::Text(text) if text.trim().is_empty() => {
                return Err(ModelError::coded(
                    "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_EMPTY_TEXT",
                    "Embedding text content must not be empty",
                    Some(format!("model={} index={index}", info.reference)),
                ));
            }
            Content::Image(image) if image.data.is_empty() => {
                return Err(ModelError::coded(
                    "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_EMPTY_IMAGE",
                    "Embedding image content must not be empty",
                    Some(format!("model={} index={index}", info.reference)),
                ));
            }
            Content::Image(image)
                if info
                    .limits
                    .max_image_bytes
                    .is_some_and(|maximum| image.data.len() > maximum) =>
            {
                let maximum = info.limits.max_image_bytes.unwrap_or_default();
                return Err(ModelError::coded(
                    "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_IMAGE_TOO_LARGE",
                    "Embedding image content exceeds model limit",
                    Some(format!(
                        "model={} index={index} imageBytes={} maxImageBytes={maximum}",
                        info.reference,
                        image.data.len()
                    )),
                ));
            }
            Content::Text(_) | Content::Image(_) => {}
        }
    }
    Ok(())
}

pub(crate) fn validate_result(
    info: &EmbeddingModelInfo,
    input_count: usize,
    result: &EmbeddingResult,
) -> Result<(), ModelError> {
    if result.vectors.len() != input_count {
        return Err(ModelError::coded(
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_VECTOR_COUNT_MISMATCH",
            "Embedding model returned the wrong number of vectors",
            Some(format!(
                "model={} contentCount={input_count} vectorCount={}",
                info.reference,
                result.vectors.len()
            )),
        ));
    }
    for (vector_index, vector) in result.vectors.iter().enumerate() {
        if vector.len() != info.dimension {
            return Err(ModelError::coded(
                "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_DIMENSION_MISMATCH",
                "Embedding model returned a vector with the wrong dimension",
                Some(format!(
                    "model={} vectorIndex={vector_index} expectedDimension={} actualDimension={}",
                    info.reference,
                    info.dimension,
                    vector.len()
                )),
            ));
        }
        if let Some(value_index) = vector.iter().position(|value| !value.is_finite()) {
            return Err(ModelError::coded(
                "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_NON_FINITE_VECTOR_VALUE",
                "Embedding model returned a non-finite vector value",
                Some(format!(
                    "model={} vectorIndex={vector_index} valueIndex={value_index}",
                    info.reference
                )),
            ));
        }
    }

    let mut seen = HashSet::new();
    for &index in &result.truncated {
        if index >= input_count || !seen.insert(index) {
            return Err(ModelError::coded(
                "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_INVALID_TRUNCATED_INPUT_INDEX",
                "Embedding model returned an invalid truncated input index",
                Some(format!(
                    "model={} index={index} inputCount={input_count}",
                    info.reference
                )),
            ));
        }
    }
    Ok(())
}

const fn kind_name(kind: EmbeddingInputKind) -> &'static str {
    match kind {
        EmbeddingInputKind::Text => "text",
        EmbeddingInputKind::Image => "image",
    }
}
