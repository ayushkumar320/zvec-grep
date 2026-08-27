use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable machine-readable engine error category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    CapabilityUnavailable,
    BackendFailure,
    Cancelled,
    DeadlineExceeded,
    Closed,
    Internal,
}

/// Error returned by a [`crate::ZvecGrep`] method.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    #[error("capability unavailable: {capability}")]
    CapabilityUnavailable { capability: String },

    #[error("{backend} failed: {message}")]
    BackendFailure { backend: String, message: String },

    #[error("request cancelled")]
    Cancelled,

    #[error("request deadline exceeded")]
    DeadlineExceeded,

    #[error("service is closed")]
    Closed,

    #[error("internal error: {message}")]
    Internal { message: String },
}

impl EngineError {
    #[must_use]
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn backend(backend: impl Into<String>, message: impl Into<String>) -> Self {
        Self::BackendFailure {
            backend: backend.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidInput { .. } => ErrorCode::InvalidInput,
            Self::CapabilityUnavailable { .. } => ErrorCode::CapabilityUnavailable,
            Self::BackendFailure { .. } => ErrorCode::BackendFailure,
            Self::Cancelled => ErrorCode::Cancelled,
            Self::DeadlineExceeded => ErrorCode::DeadlineExceeded,
            Self::Closed => ErrorCode::Closed,
            Self::Internal { .. } => ErrorCode::Internal,
        }
    }
}
