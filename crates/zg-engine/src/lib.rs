//! Shared zvec-grep engine.
//!
//! CLI, HTTP and MCP are adapters around this crate. Search policy and native
//! dependency selection must not leak into those adapters.

mod config;
mod control;
mod domain;
mod error;
mod executor;
mod ports;
mod service;

pub use config::{CoreConfig, CorePorts, ResourceBudget};
pub use control::{
    CoreEvent, CoreEventKind, EmitError, EventSink, NoopEventSink, Principal, RunControl,
    TraceContext,
};
pub use domain::*;
pub use error::{CoreError, ErrorCode, ErrorReply};
pub use executor::OperationExecutor;
pub use ports::*;
pub use service::Core;
