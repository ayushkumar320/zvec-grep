use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable machine-readable error category shared by every transport.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    UnsupportedProtocol,
    CapabilityUnavailable,
    BackendFailure,
    Cancelled,
    DeadlineExceeded,
    ShuttingDown,
    Internal,
}

/// Transport-owned representation of a Core failure.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErrorReply {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    #[error("unsupported protocol version {actual}; expected {expected}")]
    UnsupportedProtocol { actual: u32, expected: u32 },

    #[error("capability unavailable: {capability}")]
    CapabilityUnavailable { capability: String },

    #[error("{backend} failed: {message}")]
    BackendFailure { backend: String, message: String },

    #[error("operation cancelled")]
    Cancelled,

    #[error("operation deadline exceeded")]
    DeadlineExceeded,

    #[error("core is shutting down")]
    ShuttingDown,

    #[error("internal error: {message}")]
    Internal { message: String },
}

impl CoreError {
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
            Self::UnsupportedProtocol { .. } => ErrorCode::UnsupportedProtocol,
            Self::CapabilityUnavailable { .. } => ErrorCode::CapabilityUnavailable,
            Self::BackendFailure { .. } => ErrorCode::BackendFailure,
            Self::Cancelled => ErrorCode::Cancelled,
            Self::DeadlineExceeded => ErrorCode::DeadlineExceeded,
            Self::ShuttingDown => ErrorCode::ShuttingDown,
            Self::Internal { .. } => ErrorCode::Internal,
        }
    }

    #[must_use]
    pub fn to_reply(&self) -> ErrorReply {
        ErrorReply {
            code: self.code(),
            message: self.to_string(),
            retryable: false,
        }
    }
}
