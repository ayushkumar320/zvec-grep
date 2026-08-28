//! Context query requests and replies.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{ContentRange, DiscoveryOptions, EntityMetadata, SymbolType, TimingEntry};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct QueryRequest {
    /// Workspace root. `None` uses the process working directory.
    pub root: Option<PathBuf>,
    pub queries: Vec<String>,
    pub routes: Vec<QueryRoute>,
    pub fuse: bool,
    pub limit: Option<usize>,
    pub refresh: RefreshMode,
    pub trace: bool,
    pub prefer_symbol: bool,
    pub symbol_types: Vec<SymbolType>,
    pub discovery: DiscoveryOptions,
    pub modified_after_epoch_ms: Option<u64>,
    pub modified_before_epoch_ms: Option<u64>,
    /// Maximum embedding batch tasks for this query operation.
    /// The model default is used when omitted.
    pub embedding_concurrency: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QueryRoute {
    pub mode: QueryRouteMode,
    pub query: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryRouteMode {
    Fts,
    Vector,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMode {
    Background,
    Wait,
    #[default]
    Off,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueryReply {
    pub query: String,
    pub root: PathBuf,
    pub source: QuerySource,
    pub coverage: QueryCoverage,
    pub workspace_index: Option<WorkspaceIndexRef>,
    pub items: Vec<QueryItem>,
    pub diagnostics: QueryDiagnostics,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuerySource {
    Index,
    Lexical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryCoverage {
    RankedSample,
    LexicalExhaustive,
    LexicalTruncated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceIndexRef {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub generation: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueryItem {
    pub kind: QueryItemKind,
    pub rank: usize,
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub range: ContentRange,
    pub excerpt_range: Option<ContentRange>,
    pub content: String,
    pub outline: Option<String>,
    pub freshness: Freshness,
    pub score: Option<f64>,
    pub matched_by: MatchedBy,
    pub metadata: Option<EntityMetadata>,
    pub entity_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryItemKind {
    IndexedEntity,
    LexicalMatch,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Fresh,
    PossiblyStale,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedBy {
    Fts,
    Vector,
    FtsAndVector,
    Lexical,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct QueryDiagnostics {
    pub empty_reason: Option<EmptyReason>,
    pub hits_returned: usize,
    pub timings: Vec<TimingEntry>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmptyReason {
    NoMatches,
    NoSearchableFiles,
}
