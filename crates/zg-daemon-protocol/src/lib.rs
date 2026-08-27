//! Versioned wire envelopes shared by the resident daemon and thin clients.
//!
//! Framing and the local transport remain adapter concerns. These DTOs preserve
//! Core operations, outcomes, stable failures, cancellation and progress across
//! the process seam.

use serde::{Deserialize, Serialize};
use zg_engine::{CoreEvent, ErrorReply, Operation, OperationId, Outcome, Principal, TraceContext};

pub const CURRENT_DAEMON_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonRequest {
    pub message_id: u64,
    pub kind: DaemonRequestKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum DaemonRequestKind {
    Hello(HelloRequest),
    Execute(Box<ExecuteRequest>),
    Cancel(CancelRequest),
    Shutdown(ShutdownRequest),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HelloRequest {
    pub protocol_version: u32,
    pub client_version: String,
}

impl HelloRequest {
    #[must_use]
    pub fn current(client_version: impl Into<String>) -> Self {
        Self {
            protocol_version: CURRENT_DAEMON_PROTOCOL_VERSION,
            client_version: client_version.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExecuteRequest {
    pub operation: Operation,
    /// Remaining duration at the client, avoiding cross-process `Instant`
    /// serialization and wall-clock skew.
    pub timeout_millis: Option<u64>,
    pub principal: Principal,
    pub trace: TraceContext,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelRequest {
    pub operation_id: OperationId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShutdownRequest {
    pub grace_period_millis: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DaemonResponse {
    pub message_id: u64,
    pub kind: DaemonResponseKind,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum DaemonResponseKind {
    Hello(HelloReply),
    Event(CoreEvent),
    Result(ExecuteResult),
    Acknowledged(DaemonAction),
    Error(ErrorReply),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HelloReply {
    pub protocol_version: u32,
    pub daemon_version: String,
    pub daemon_instance_id: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonAction {
    Cancel,
    Shutdown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ExecuteResult {
    pub operation_id: OperationId,
    pub result: ExecutionResult,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "value")]
pub enum ExecutionResult {
    Success(Outcome),
    Failure(ErrorReply),
}

impl ExecutionResult {
    #[must_use]
    pub fn from_result(result: Result<Outcome, ErrorReply>) -> Self {
        match result {
            Ok(outcome) => Self::Success(outcome),
            Err(error) => Self::Failure(error),
        }
    }

    /// Converts the wire result back to the transport execution result.
    ///
    /// # Errors
    ///
    /// Returns the stable Core failure carried by a Failure frame.
    pub fn into_result(self) -> Result<Outcome, ErrorReply> {
        match self {
            Self::Success(outcome) => Ok(outcome),
            Self::Failure(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zg_engine::{Command, Operation, QueryRequest};

    use super::{
        CURRENT_DAEMON_PROTOCOL_VERSION, DaemonRequest, DaemonRequestKind, ExecuteRequest,
        HelloRequest,
    };

    #[test]
    fn hello_uses_current_protocol_version() {
        let hello = HelloRequest::current("fixture-client");
        assert_eq!(hello.protocol_version, CURRENT_DAEMON_PROTOCOL_VERSION);
    }

    #[test]
    fn execute_request_round_trips_without_run_control() {
        let request = DaemonRequest {
            message_id: 7,
            kind: DaemonRequestKind::Execute(Box::new(ExecuteRequest {
                operation: Operation::new(
                    PathBuf::from("/workspace"),
                    Command::Query(QueryRequest::default()),
                ),
                timeout_millis: Some(5_000),
                principal: zg_engine::Principal::local(),
                trace: zg_engine::TraceContext {
                    trace_id: Some("trace-1".to_owned()),
                },
            })),
        };
        let encoded = serde_json::to_string(&request).expect("daemon request should serialize");
        let decoded: DaemonRequest =
            serde_json::from_str(&encoded).expect("daemon request should deserialize");
        assert_eq!(decoded, request);
    }
}
