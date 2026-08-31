use thiserror::Error;

/// Failure produced by native filesystem scanning or watching.
#[derive(Debug, Error)]
pub enum HostError {
    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    #[error("{backend} failed: {message}")]
    BackendFailure { backend: String, message: String },

    #[error("operation cancelled")]
    Cancelled,

    #[error("operation deadline exceeded")]
    DeadlineExceeded,

    #[error("native host resource is closed")]
    Closed,

    #[error("internal error: {message}")]
    Internal { message: String },
}

impl HostError {
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
}
