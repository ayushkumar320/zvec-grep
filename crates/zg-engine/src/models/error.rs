use thiserror::Error;

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ModelError {
    code: Option<&'static str>,
    message: String,
    context: Option<String>,
    cause: Option<String>,
}

impl ModelError {
    pub(crate) fn coded(
        code: &'static str,
        message: impl Into<String>,
        context: Option<String>,
    ) -> Self {
        Self {
            code: Some(code),
            message: message.into(),
            context,
            cause: None,
        }
    }

    pub(crate) fn uncoded(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
            context: None,
            cause: None,
        }
    }

    pub(crate) fn with_cause(mut self, cause: impl std::fmt::Display) -> Self {
        self.cause = Some(cause.to_string());
        self
    }

    /// Stable TypeScript `EngineError` code when the source behavior assigns one.
    #[must_use]
    pub const fn code(&self) -> Option<&'static str> {
        self.code
    }

    #[must_use]
    pub fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    #[must_use]
    pub fn cause(&self) -> Option<&str> {
        self.cause.as_deref()
    }
}
