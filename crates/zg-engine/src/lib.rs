//! High-level Rust API for zvec-grep.
//!
//! Create one [`ZvecGrep`] for a process and call its typed methods. Command
//! dispatch, transport envelopes and adapter composition are not part of this
//! public API.

mod domain;
mod error;
mod lexical;
#[allow(dead_code)]
mod models;
mod service;

pub use domain::{
    ChangeIndexAction, ChangeIndexReply, ChangeIndexRequest, Content, ContentRange, Device,
    DiscoveryOptions, EmbeddingInputKind, EmbeddingMetric, EmbeddingModelSpec, EmptyReason,
    EntityMetadata, FileKind, Freshness, ImageContent, ImageFormat, IndexPolicy, IndexReply,
    IndexRequest, InspectReply, InspectRequest, InspectSource, JobAction, JobInfo, JobReceipt,
    JobReply, JobRequest, JobState, LexicalCoverage, LexicalDiagnostics, LexicalMatch,
    LexicalOptions, LexicalSearchReply, LexicalSearchRequest, ManagedRgArgumentError, MatchedBy,
    QueryCoverage, QueryDiagnostics, QueryItem, QueryItemKind, QueryReply, QueryRequest,
    QueryRoute, QueryRouteMode, QuerySource, RefreshMode, RootSpec, SkippedFile, SkippedFileReason,
    SymbolType, TextRange, TimingEntry, WorkspaceIndexInfo, WorkspaceIndexRef,
    WorkspaceIndexStatus, parse_managed_rg_args,
};
pub use error::{EngineError, ErrorCode};
pub use service::ZvecGrep;
