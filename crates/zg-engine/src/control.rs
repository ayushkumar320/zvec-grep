use std::{fmt, sync::Arc, time::Instant};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{ErrorCode, OperationId};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Principal {
    pub subject: String,
}

impl Principal {
    #[must_use]
    pub fn local() -> Self {
        Self {
            subject: "local-user".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TraceContext {
    pub trace_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CoreEvent {
    pub operation_id: OperationId,
    pub sequence: u64,
    pub kind: CoreEventKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CoreEventKind {
    Started,
    Progress { completed: u64, total: Option<u64> },
    Warning { code: String, message: String },
    Completed { result_count: usize },
    Failed { code: ErrorCode },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmitError;

/// A non-blocking event boundary. Implementations must enqueue with `try_send`
/// semantics and may drop progress events under backpressure.
pub trait EventSink: Send + Sync {
    /// Attempts to enqueue an event without waiting.
    ///
    /// # Errors
    ///
    /// Returns [`EmitError`] when the consumer cannot accept the event. Core
    /// execution continues because telemetry backpressure is not operational
    /// backpressure.
    fn try_emit(&self, event: CoreEvent) -> Result<(), EmitError>;
}

#[derive(Debug, Default)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn try_emit(&self, _event: CoreEvent) -> Result<(), EmitError> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct RunControl {
    pub cancellation: CancellationToken,
    pub deadline: Option<Instant>,
    pub events: Arc<dyn EventSink>,
    pub principal: Principal,
    pub trace: TraceContext,
}

impl RunControl {
    #[must_use]
    pub fn local(cancellation: CancellationToken) -> Self {
        Self {
            cancellation,
            deadline: None,
            events: Arc::new(NoopEventSink),
            principal: Principal::local(),
            trace: TraceContext::default(),
        }
    }
}

impl fmt::Debug for RunControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunControl")
            .field("cancelled", &self.cancellation.is_cancelled())
            .field("deadline", &self.deadline)
            .field("principal", &self.principal)
            .field("trace", &self.trace)
            .finish_non_exhaustive()
    }
}
