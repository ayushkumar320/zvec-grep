use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    ChangeIndexReply, ChangeIndexRequest, IndexReply, IndexRequest, InspectReply, InspectRequest,
    JobReceipt, JobReply, JobRequest, LexicalSearchReply, LexicalSearchRequest, QueryReply,
    QueryRequest,
};

pub const CURRENT_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OperationId(Uuid);

impl OperationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Operation {
    pub protocol_version: u32,
    pub id: OperationId,
    pub root: PathBuf,
    pub command: Command,
    pub authorization: Option<AuthorizationProof>,
}

impl Operation {
    #[must_use]
    pub fn new(root: PathBuf, command: Command) -> Self {
        Self {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            id: OperationId::new(),
            root,
            command,
            authorization: None,
        }
    }

    #[must_use]
    pub fn lexical(root: PathBuf, request: LexicalSearchRequest) -> Self {
        Self::new(root, Command::LexicalSearch(request))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "request")]
pub enum Command {
    Query(QueryRequest),
    LexicalSearch(LexicalSearchRequest),
    Index(IndexRequest),
    Inspect(InspectRequest),
    ChangeIndex(ChangeIndexRequest),
    Job(JobRequest),
}

impl Command {
    #[must_use]
    pub const fn capability(&self) -> &'static str {
        match self {
            Self::Query(_) => "query",
            Self::LexicalSearch(_) => "lexical_search",
            Self::Index(_) => "index",
            Self::Inspect(_) => "inspect",
            Self::ChangeIndex(_) => "change_index",
            Self::Job(_) => "job",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "reply")]
pub enum Reply {
    Query(Box<QueryReply>),
    LexicalSearch(Box<LexicalSearchReply>),
    Index(Box<IndexReply>),
    Inspect(Box<InspectReply>),
    ChangeIndex(Box<ChangeIndexReply>),
    Job(Box<JobReply>),
}

impl Reply {
    #[must_use]
    pub fn result_count(&self) -> usize {
        match self {
            Self::Query(reply) => reply.items.len(),
            Self::LexicalSearch(reply) => reply.matches.len(),
            Self::Index(reply) => reply.entities_created,
            Self::Inspect(_) | Self::ChangeIndex(_) => 1,
            Self::Job(reply) => reply.jobs.len(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "value")]
pub enum Outcome {
    Completed(Box<Reply>),
    Accepted(JobReceipt),
    InputRequired(AuthorizationChallenge),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorizationChallenge {
    pub operation_digest: String,
    pub reason: String,
    pub choices: Vec<AuthorizationChoice>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationChoice {
    AllowOnce,
    AllowWorkspace,
    UseFtsOnly,
    Cancel,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorizationProof {
    pub operation_digest: String,
    pub token: String,
}
