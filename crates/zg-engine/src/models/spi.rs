//! Backend contract shared by the model runtime and embedding implementations.

use std::{collections::HashSet, fmt, path::PathBuf, sync::Arc};

use crate::{api::index::options::Device, payload::Content};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::compute::ModelComputeRuntime;
pub(super) use super::error::ModelError;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EmbeddingMetric {
    Cosine,
    DotProduct,
    Euclidean,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EmbeddingInputKind {
    Text,
    Image,
}

#[derive(Clone, Default)]
pub struct CreateEmbeddingModelOptions {
    pub api_key: Option<String>,
    pub endpoint: Option<String>,
    pub model_cache_dir: Option<PathBuf>,
    pub device: Option<Device>,
    pub(crate) compute_runtime: Option<ModelComputeRuntime>,
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
    /// Runtime-owned execution budget. Backends use this to size their
    /// per-request local inference resources without exposing another public
    /// tuning knob.
    pub(crate) execution_concurrency: usize,
    /// Standard W3C trace headers propagated by remote embedding backends.
    pub(crate) trace_headers: Option<EmbeddingTraceHeaders>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EmbeddingTraceHeaders {
    pub(crate) traceparent: String,
    pub(crate) tracestate: Option<String>,
    pub(crate) baggage: Option<String>,
}

impl fmt::Debug for EmbeddingOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbeddingOptions")
            .field("purpose", &self.purpose)
            .field("has_signal", &self.signal.is_some())
            .field("has_progress_callback", &self.on_progress.is_some())
            .field("execution_concurrency", &self.execution_concurrency)
            .field("has_trace_headers", &self.trace_headers.is_some())
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

#[cfg(test)]
mod tests {
    use crate::payload::{Content, ImageContent, ImageFormat};

    use super::*;

    #[test]
    fn validates_all_representable_input_failures_from_the_typescript_base_class() {
        let mut info = fixture_info();

        assert_error_code(
            validate_contents(&info, &[]),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_EMPTY_INPUT",
        );
        assert_error_code(
            validate_contents(
                &info,
                &[
                    Content::Text("one".to_owned()),
                    Content::Text("two".to_owned()),
                    Content::Text("three".to_owned()),
                ],
            ),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_BATCH_TOO_LARGE",
        );
        assert_error_code(
            validate_contents(&info, &[Content::Text("  ".to_owned())]),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_EMPTY_TEXT",
        );
        assert_error_code(
            validate_contents(
                &info,
                &[Content::Image(ImageContent {
                    data: Vec::new(),
                    format: ImageFormat::Png,
                })],
            ),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_EMPTY_IMAGE",
        );
        assert_error_code(
            validate_contents(
                &info,
                &[Content::Image(ImageContent {
                    data: vec![1, 2, 3, 4],
                    format: ImageFormat::Png,
                })],
            ),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_IMAGE_TOO_LARGE",
        );

        info.input_kinds = vec![EmbeddingInputKind::Text];
        assert_error_code(
            validate_contents(
                &info,
                &[Content::Image(ImageContent {
                    data: vec![1],
                    format: ImageFormat::Png,
                })],
            ),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_UNSUPPORTED_CONTENT",
        );
    }

    #[test]
    fn validates_all_representable_provider_output_failures() {
        let info = fixture_info();

        assert_error_code(
            validate_result(
                &info,
                1,
                &EmbeddingResult {
                    vectors: Vec::new(),
                    truncated: Vec::new(),
                },
            ),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_VECTOR_COUNT_MISMATCH",
        );
        assert_error_code(
            validate_result(
                &info,
                1,
                &EmbeddingResult {
                    vectors: vec![vec![1.0]],
                    truncated: Vec::new(),
                },
            ),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_DIMENSION_MISMATCH",
        );
        assert_error_code(
            validate_result(
                &info,
                1,
                &EmbeddingResult {
                    vectors: vec![vec![1.0, f32::NAN]],
                    truncated: Vec::new(),
                },
            ),
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_NON_FINITE_VECTOR_VALUE",
        );
        for truncated in [vec![1], vec![0, 0]] {
            assert_error_code(
                validate_result(
                    &info,
                    1,
                    &EmbeddingResult {
                        vectors: vec![vec![1.0, 0.0]],
                        truncated,
                    },
                ),
                "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_INVALID_TRUNCATED_INPUT_INDEX",
            );
        }
    }

    fn fixture_info() -> EmbeddingModelInfo {
        EmbeddingModelInfo {
            reference: "test/stub".to_owned(),
            provider: "test".to_owned(),
            name: "stub".to_owned(),
            dimension: 2,
            metric: EmbeddingMetric::Cosine,
            endpoint: None,
            default_concurrency: Some(2),
            input_kinds: vec![EmbeddingInputKind::Text, EmbeddingInputKind::Image],
            limits: EmbeddingModelLimits {
                max_batch_size: 2,
                max_input_tokens: None,
                max_image_bytes: Some(3),
            },
        }
    }

    fn assert_error_code(result: Result<(), ModelError>, expected: &'static str) {
        assert_eq!(
            result
                .expect_err("validation should reject the fixture")
                .code(),
            Some(expected)
        );
    }
}
