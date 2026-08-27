//! MCP transport adapter for the public agent toolset.
//!
//! This crate owns MCP schemas and formatting only. Every request becomes a
//! canonical [`zg_engine::Operation`] and executes through an injected
//! [`zg_engine::OperationExecutor`].

use std::{fmt::Write as _, path::PathBuf, sync::Arc};

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo},
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use zg_engine::{
    Command, ContentRange, ErrorCode, Freshness, MatchedBy, Operation, OperationExecutor, Outcome,
    QueryItem, QueryReply, QueryRequest, QueryRoute, QueryRouteMode, RefreshMode, Reply,
    RunControl, SymbolType,
};

pub const AGENT_TOOL_NAME: &str = "zvec_grep_search";

pub const AGENT_INSTRUCTIONS: &str = concat!(
    "Use zvec-grep with these workspace retrieval rules:\n",
    "- Use the current workspace as the evidence source when the user asks about local material, prior context establishes it as relevant, or the question concerns how the current project works—even if the workspace is not mentioned explicitly.\n",
    "- A workspace may contain any mix of code, documents, configuration, and data.\n",
    "- Do not use workspace retrieval for unrelated open-world questions, current external facts, or web content that does not depend on local evidence.\n",
    "- Use native Grep or rg first only when exact lookup alone is sufficient, such as locating one definition, literal, filename, configuration key, error message, regex match, or exhaustive occurrence list.\n",
    "- Use zvec_grep_search first when wording or location is unknown, or when the answer requires architecture, lifecycle, call relationships, dependencies, data or control flow, design rationale, comparison, or synthesis across files or components.\n",
    "- When user-provided or verified exact symbols are present but the answer spans multiple files, components, stages, implementations, or relationships, treat the task as mixed: call zvec_grep_search with the semantic intent and those anchors, then use Read, Grep, or rg for focused verification.\n",
    "- For a semantic or mixed workspace task, start discovery with focused zvec_grep_search before broad file discovery.\n",
    "- Preserve the question's concepts, relationships, and constraints from the user request and established context in semantic queries. Treat inferred names as supplemental hypotheses, not replacements for or constraints on the stated intent.\n",
    "- `query` creates one primary hybrid FTS-plus-vector group; `queries` creates one or more primary hybrid groups; `fts` and `vector` add supplemental lexical-only or semantic-only route groups. These are retrieval routes, not hard constraints. Without `fuse`, the response is one deduplicated and reranked list with query-group metadata; set `fuse: true` to collapse every group into one ranked search plan.\n",
    "- For a fused mixed search, use arguments such as {\"root\":\"/absolute/workspace\",\"query\":\"how are results ranked and fused\",\"fts\":[\"RRF\",\"score\"],\"fuse\":true}.\n",
    "- Search results include bounded source snippets. Treat a sufficient snippet as already-read evidence, and open only the cited file or range when a required detail falls outside it.\n",
    "- If semantic retrieval remains irrelevant, fall back to native Grep or rg.\n",
    "- Stop searching once the available evidence is sufficient for the requested task. Continue only to resolve a material gap or ambiguity; do not repeat similar searches or broaden the investigation merely to reconfirm what is already established.\n",
    "- Do not launch a sub-agent solely to locate workspace material.\n",
    "- Every workspace operation requires an absolute root path visible to the daemon.\n",
    "- Read freshness and background_refresh directly from zvec_grep_search responses without a status preflight.\n",
    "- When results are served_from_current_index, use them immediately when they are sufficient; do not perform extra diagnostics merely because a background refresh is active.\n",
    "- When an index is missing and literal or regex search can answer the task, use native Grep or rg. Creating or rebuilding a persistent index requires explicit user authorization.",
);

const MAX_QUERY_GROUPS: usize = 32;
const MAX_QUERY_CHARS: usize = 4_000;
const MAX_PATH_FILTERS: usize = 128;
const MAX_PATH_CHARS: usize = 1_024;
const MAX_SEARCH_LIMIT: usize = 50;

#[derive(Clone)]
pub struct AgentMcpServer {
    executor: Arc<dyn OperationExecutor>,
}

impl AgentMcpServer {
    #[must_use]
    pub fn new(executor: Arc<dyn OperationExecutor>) -> Self {
        Self { executor }
    }
}

#[tool_router]
impl AgentMcpServer {
    #[tool(
        name = "zvec_grep_search",
        description = "Search an existing workspace index for semantic, relational, cross-file, or multi-hop evidence such as architecture, call chains, dependencies, lifecycle, data or control flow, design rationale, and comparisons. Use it when exact lookup alone cannot answer a workspace-grounded question. Results include bounded source snippets and query-group metadata; treat sufficient snippets as already-read evidence. Use native Grep or rg instead when exact lookup alone is sufficient. Read freshness and background_refresh from the response without a status preflight; when results are served_from_current_index, use them if sufficient.",
        annotations(
            title = "Search with zvec-grep",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn zvec_grep_search(
        &self,
        Parameters(input): Parameters<SearchInput>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let operation = input
            .into_operation()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let control = RunControl::local(context.ct.child_token());
        let result = self.executor.execute(operation, control).await;

        Ok(match result {
            Ok(outcome) => outcome_to_result(outcome),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                "error_code: {}\nerror_message: {}\nretryable: {}",
                error_code_label(error.code),
                error.message,
                error.retryable
            ))]),
        })
    }
}

#[tool_handler]
#[allow(clippy::unused_async_trait_impl)]
impl ServerHandler for AgentMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                "zvec-grep",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(AGENT_INSTRUCTIONS.to_owned())
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchInput {
    /// Absolute workspace root visible to the daemon.
    #[schemars(length(min = 1, max = 1024))]
    pub root: String,
    /// One-request embedding provider API key override.
    #[schemars(length(min = 1, max = 8192))]
    pub api_key: Option<String>,
    /// One-request local embedding device override.
    pub device: Option<DeviceInput>,
    /// One primary hybrid-search group.
    #[schemars(length(max = 4000))]
    pub query: Option<String>,
    /// One or more primary hybrid-search groups.
    pub queries: Option<QueryListInput>,
    /// Supplemental lexical-route groups.
    pub fts: Option<QueryListInput>,
    /// Supplemental semantic/vector-route groups.
    pub vector: Option<QueryListInput>,
    /// Maximum returned items per query group or fused plan.
    #[schemars(range(min = 1, max = 50))]
    pub limit: Option<usize>,
    /// Ordered case-sensitive rg-style glob rules.
    pub globs: Option<PathListInput>,
    /// Ordered case-insensitive rg-style glob rules.
    pub insensitive_globs: Option<PathListInput>,
    /// Included ripgrep file type names.
    pub file_types: Option<PathListInput>,
    /// Excluded ripgrep file type names.
    pub excluded_file_types: Option<PathListInput>,
    /// Include hidden paths.
    pub hidden: Option<bool>,
    /// Ignore repository ignore files.
    pub no_ignore: Option<bool>,
    /// Additional ignore files relative to the workspace root.
    pub ignore_files: Option<PathListInput>,
    /// Maximum recursive directory depth.
    pub max_depth: Option<usize>,
    /// Maximum indexed file size in bytes.
    #[schemars(range(min = 1))]
    pub max_file_size_bytes: Option<u64>,
    /// Follow symbolic links.
    pub follow: Option<bool>,
    /// Embedding requests processed concurrently during updates.
    #[schemars(range(min = 1))]
    pub embedding_concurrency: Option<usize>,
    /// Collapse all query groups into one ranked plan.
    pub fuse: Option<bool>,
    /// Prefer exact indexed symbols.
    pub prefer_symbol: Option<bool>,
    /// Restrict indexed results to symbol types.
    #[serde(default)]
    #[schemars(length(max = 6))]
    pub symbol_types: Vec<SymbolTypeInput>,
    /// Only query files modified after this time.
    pub modified_after: Option<TimeInput>,
    /// Only query files modified before this time.
    pub modified_before: Option<TimeInput>,
    /// Include per-hit search trace.
    pub trace: Option<bool>,
    /// Search now or wait for the active index to become fresh.
    #[serde(default)]
    pub freshness: FreshnessInput,
    /// Allow eventual search to schedule a background index update.
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum QueryListInput {
    One(#[schemars(length(max = 4000))] String),
    Many(#[schemars(length(max = 32), inner(length(max = 4000)))] Vec<String>),
}

impl QueryListInput {
    fn normalized(self, name: &str) -> Result<Vec<String>, String> {
        let values = match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        };
        if values.len() > MAX_QUERY_GROUPS {
            return Err(format!("{name} accepts at most {MAX_QUERY_GROUPS} values"));
        }
        Ok(values
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect())
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PathListInput {
    One(#[schemars(length(max = 1024))] String),
    Many(#[schemars(length(max = 128), inner(length(max = 1024)))] Vec<String>),
}

impl PathListInput {
    fn normalized(self, name: &str) -> Result<Vec<String>, String> {
        let values = match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        };
        if values.len() > MAX_PATH_FILTERS {
            return Err(format!("{name} accepts at most {MAX_PATH_FILTERS} values"));
        }
        Ok(values
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceInput {
    Auto,
    Cpu,
    Metal,
    Vulkan,
    Cuda,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SymbolTypeInput {
    Module,
    Class,
    Interface,
    Function,
    Value,
    Alias,
}

impl From<SymbolTypeInput> for SymbolType {
    fn from(value: SymbolTypeInput) -> Self {
        match value {
            SymbolTypeInput::Module => Self::Module,
            SymbolTypeInput::Class => Self::Class,
            SymbolTypeInput::Interface => Self::Interface,
            SymbolTypeInput::Function => Self::Function,
            SymbolTypeInput::Value => Self::Value,
            SymbolTypeInput::Alias => Self::Alias,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessInput {
    #[default]
    Eventual,
    WaitForFresh,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TimeInput {
    EpochMillis(u64),
    Text(#[schemars(length(max = 128))] String),
}

const fn default_auto_update() -> bool {
    true
}

impl SearchInput {
    fn into_operation(self) -> Result<Operation, String> {
        validate_text("root", &self.root, 1, MAX_PATH_CHARS)?;
        let root = PathBuf::from(self.root.trim());
        if !root.is_absolute() {
            return Err("root must be an absolute path".to_owned());
        }
        if let Some(api_key) = &self.api_key {
            validate_text("apiKey", api_key, 1, 8_192)?;
            return Err(
                "apiKey is not available until remote embedding authorization is implemented"
                    .to_owned(),
            );
        }
        if self.device.is_some() {
            return Err(
                "device is not available until the local embedding runtime is implemented"
                    .to_owned(),
            );
        }
        if self
            .limit
            .is_some_and(|limit| limit == 0 || limit > MAX_SEARCH_LIMIT)
        {
            return Err(format!("limit must be between 1 and {MAX_SEARCH_LIMIT}"));
        }
        if self.embedding_concurrency == Some(0) {
            return Err("embeddingConcurrency must be greater than zero".to_owned());
        }
        if self.max_file_size_bytes == Some(0) {
            return Err("maxFileSizeBytes must be greater than zero".to_owned());
        }
        if self.symbol_types.len() > 6 {
            return Err("symbolTypes accepts at most 6 values".to_owned());
        }

        let mut queries = normalize_optional_one(self.query, "query")?;
        queries.extend(normalize_query_list(self.queries, "queries")?);
        let fts = normalize_query_list(self.fts, "fts")?;
        let vector = normalize_query_list(self.vector, "vector")?;
        if queries.is_empty() && fts.is_empty() && vector.is_empty() {
            return Err("zvec_grep_search requires query, queries, fts, or vector".to_owned());
        }

        let routes = fts
            .into_iter()
            .map(|query| QueryRoute {
                mode: QueryRouteMode::Fts,
                query,
            })
            .chain(vector.into_iter().map(|query| QueryRoute {
                mode: QueryRouteMode::Vector,
                query,
            }))
            .collect();
        let refresh = match (self.freshness, self.auto_update) {
            (FreshnessInput::WaitForFresh, _) => RefreshMode::Wait,
            (FreshnessInput::Eventual, true) => RefreshMode::Background,
            (FreshnessInput::Eventual, false) => RefreshMode::Off,
        };

        let request = QueryRequest {
            queries,
            routes,
            fuse: self.fuse.unwrap_or(false),
            limit: self.limit,
            refresh,
            trace: self.trace.unwrap_or(false),
            prefer_symbol: self.prefer_symbol.unwrap_or(false),
            symbol_types: self.symbol_types.into_iter().map(Into::into).collect(),
            discovery: zg_engine::DiscoveryOptions {
                globs: normalize_path_list(self.globs, "globs")?,
                insensitive_globs: normalize_path_list(self.insensitive_globs, "insensitiveGlobs")?,
                file_types: normalize_path_list(self.file_types, "fileTypes")?,
                excluded_file_types: normalize_path_list(
                    self.excluded_file_types,
                    "excludedFileTypes",
                )?,
                hidden: self.hidden.unwrap_or(false),
                no_ignore: self.no_ignore.unwrap_or(false),
                ignore_files: normalize_path_list(self.ignore_files, "ignoreFiles")?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect(),
                max_depth: self.max_depth,
                max_file_size_bytes: self.max_file_size_bytes,
                follow: self.follow.unwrap_or(false),
                ..zg_engine::DiscoveryOptions::default()
            },
            modified_after_epoch_ms: parse_optional_time(self.modified_after, "modifiedAfter")?,
            modified_before_epoch_ms: parse_optional_time(self.modified_before, "modifiedBefore")?,
            embedding_concurrency: self.embedding_concurrency,
        };
        if request
            .modified_after_epoch_ms
            .zip(request.modified_before_epoch_ms)
            .is_some_and(|(after, before)| after > before)
        {
            return Err("modifiedAfter must not be later than modifiedBefore".to_owned());
        }
        Ok(Operation::new(root, Command::Query(request)))
    }
}

fn normalize_optional_one(value: Option<String>, name: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    validate_text(name, &value, 0, MAX_QUERY_CHARS)?;
    let value = value.trim();
    Ok((!value.is_empty())
        .then(|| value.to_owned())
        .into_iter()
        .collect())
}

fn normalize_query_list(value: Option<QueryListInput>, name: &str) -> Result<Vec<String>, String> {
    let values = value.map_or_else(|| Ok(Vec::new()), |value| value.normalized(name))?;
    for value in &values {
        validate_text(name, value, 0, MAX_QUERY_CHARS)?;
    }
    Ok(values)
}

fn normalize_path_list(value: Option<PathListInput>, name: &str) -> Result<Vec<String>, String> {
    let values = value.map_or_else(|| Ok(Vec::new()), |value| value.normalized(name))?;
    for value in &values {
        validate_text(name, value, 0, MAX_PATH_CHARS)?;
    }
    Ok(values)
}

fn validate_text(name: &str, value: &str, min: usize, max: usize) -> Result<(), String> {
    let length = value.chars().count();
    if length < min || length > max {
        return Err(format!(
            "{name} must contain between {min} and {max} characters"
        ));
    }
    Ok(())
}

fn parse_optional_time(value: Option<TimeInput>, name: &str) -> Result<Option<u64>, String> {
    value.map(|value| parse_time(value, name)).transpose()
}

fn parse_time(value: TimeInput, name: &str) -> Result<u64, String> {
    if let TimeInput::EpochMillis(value) = value {
        return Ok(value);
    }
    let TimeInput::Text(value) = value else {
        unreachable!();
    };
    validate_text(name, &value, 0, 128)?;
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value
            .parse()
            .map_err(|_| format!("{name} requires a valid epoch millisecond value"));
    }
    if let Ok(value) = DateTime::parse_from_rfc3339(&value) {
        return epoch_millis(value.timestamp_millis(), name);
    }
    if let Ok(value) = DateTime::parse_from_rfc2822(&value) {
        return epoch_millis(value.timestamp_millis(), name);
    }
    if let Ok(date) = NaiveDate::parse_from_str(&value, "%Y-%m-%d") {
        let local = date
            .and_hms_opt(0, 0, 0)
            .and_then(|date_time| {
                Local
                    .from_local_datetime(&date_time)
                    .single()
                    .or_else(|| Local.from_local_datetime(&date_time).earliest())
            })
            .ok_or_else(|| format!("{name} is not a valid local date"))?;
        return epoch_millis(local.timestamp_millis(), name);
    }
    Err(format!(
        "{name} requires an epoch millisecond value or an RFC 3339/RFC 2822/date value"
    ))
}

fn epoch_millis(value: i64, name: &str) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| format!("{name} must not be before the Unix epoch"))
}

fn outcome_to_result(outcome: Outcome) -> CallToolResult {
    match outcome {
        Outcome::Completed(reply) => match *reply {
            Reply::Query(reply) => {
                CallToolResult::success(vec![ContentBlock::text(format_query_reply(&reply))])
            }
            reply => CallToolResult::error(vec![ContentBlock::text(format!(
                "error_code: internal\nerror_message: query returned unexpected reply {}",
                reply_name(&reply)
            ))]),
        },
        Outcome::Accepted(receipt) => CallToolResult::success(vec![ContentBlock::text(format!(
            "job_id: {}\nstate: accepted",
            receipt.id
        ))]),
        Outcome::InputRequired(challenge) => {
            CallToolResult::error(vec![ContentBlock::text(format!(
                "error_code: authorization_required\nerror_message: {}",
                challenge.reason
            ))])
        }
    }
}

fn reply_name(reply: &Reply) -> &'static str {
    match reply {
        Reply::Query(_) => "query",
        Reply::LexicalSearch(_) => "lexical_search",
        Reply::Index(_) => "index",
        Reply::Inspect(_) => "inspect",
        Reply::ChangeIndex(_) => "change_index",
        Reply::Job(_) => "job",
    }
}

fn error_code_label(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidInput => "invalid_input",
        ErrorCode::UnsupportedProtocol => "unsupported_protocol",
        ErrorCode::CapabilityUnavailable => "capability_unavailable",
        ErrorCode::BackendFailure => "backend_failure",
        ErrorCode::Cancelled => "cancelled",
        ErrorCode::DeadlineExceeded => "deadline_exceeded",
        ErrorCode::ShuttingDown => "shutting_down",
        ErrorCode::Internal => "internal",
    }
}

fn format_query_reply(reply: &QueryReply) -> String {
    let freshness = if reply
        .items
        .iter()
        .any(|item| item.freshness == Freshness::PossiblyStale)
    {
        "possibly_stale"
    } else {
        "fresh"
    };
    let mut output = format!("freshness: {freshness}");
    if reply.items.is_empty() {
        let _ = write!(output, "\nNo matches.");
        return output;
    }
    let mut items: Vec<&QueryItem> = reply.items.iter().collect();
    items.sort_by_key(|item| item.rank);
    for item in items {
        let _ = write!(
            output,
            "\n\n#{} matchedBy={} {}:{}",
            item.rank,
            matched_by_label(item.matched_by),
            item.relative_path.display(),
            range_label(&item.range)
        );
        if let Some(outline) = &item.outline {
            for line in outline.lines().take(7) {
                let _ = write!(output, "\noutline: {}", truncate_line(line));
            }
        }
        output.push_str("\nsource:");
        for line in item.content.lines().take(10) {
            let _ = write!(output, "\n  {}", truncate_line(line));
        }
    }
    output
}

fn matched_by_label(value: MatchedBy) -> &'static str {
    match value {
        MatchedBy::Fts => "fts",
        MatchedBy::Vector => "vector",
        MatchedBy::FtsAndVector => "fts+vector",
        MatchedBy::Lexical => "lexical",
    }
}

fn range_label(range: &ContentRange) -> String {
    match range {
        ContentRange::File => "file".to_owned(),
        ContentRange::Text {
            start_line,
            end_line,
            ..
        } if start_line == end_line => start_line.to_string(),
        ContentRange::Text {
            start_line,
            end_line,
            ..
        } => format!("{start_line}-{end_line}"),
        ContentRange::Byte {
            start_offset,
            end_offset,
        } => format!("bytes:{start_offset}-{end_offset}"),
        ContentRange::Page { page } => format!("page:{page}"),
        ContentRange::PageText { page, .. } | ContentRange::PageRegion { page, .. } => {
            format!("page:{page}")
        }
    }
}

fn truncate_line(line: &str) -> String {
    const MAX_LINE_CHARS: usize = 160;
    let mut chars = line.chars();
    let prefix: String = chars.by_ref().take(MAX_LINE_CHARS).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use rmcp::ServerHandler;
    use zg_engine::{Command, RefreshMode};
    use zg_testkit::fakes::ScriptedExecutor;

    use super::{
        AGENT_TOOL_NAME, AgentMcpServer, FreshnessInput, PathListInput, QueryListInput, SearchInput,
    };

    fn input() -> SearchInput {
        SearchInput {
            root: "/workspace".to_owned(),
            api_key: None,
            device: None,
            query: Some("call chain".to_owned()),
            queries: None,
            fts: Some(QueryListInput::One("run".to_owned())),
            vector: None,
            limit: Some(8),
            globs: Some(PathListInput::Many(vec!["*.rs".to_owned()])),
            insensitive_globs: None,
            file_types: None,
            excluded_file_types: None,
            hidden: Some(true),
            no_ignore: None,
            ignore_files: None,
            max_depth: None,
            max_file_size_bytes: None,
            follow: None,
            embedding_concurrency: Some(2),
            fuse: Some(true),
            prefer_symbol: Some(true),
            symbol_types: Vec::new(),
            modified_after: None,
            modified_before: None,
            trace: Some(true),
            freshness: FreshnessInput::Eventual,
            auto_update: true,
        }
    }

    #[test]
    fn maps_agent_search_to_canonical_query_operation() {
        let operation = input().into_operation().expect("search input should map");
        assert_eq!(operation.root, PathBuf::from("/workspace"));
        let Command::Query(request) = operation.command else {
            panic!("search must map to query");
        };
        assert_eq!(request.queries, ["call chain"]);
        assert_eq!(request.routes.len(), 1);
        assert_eq!(request.refresh, RefreshMode::Background);
        assert_eq!(request.discovery.globs, ["*.rs"]);
        assert!(request.fuse);
        assert!(request.prefer_symbol);
        assert!(request.trace);
    }

    #[test]
    fn agent_server_exposes_only_search() {
        let server = AgentMcpServer::new(Arc::new(ScriptedExecutor::default()));
        let tools = AgentMcpServer::tool_router().list_all();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, AGENT_TOOL_NAME);
        assert!(server.get_info().instructions.is_some());
    }

    #[test]
    fn rejects_relative_roots_and_empty_queries() {
        let mut relative = input();
        relative.root = PathBuf::from("workspace").display().to_string();
        assert!(relative.into_operation().is_err());

        let mut empty = input();
        empty.query = Some("  ".to_owned());
        empty.fts = None;
        assert!(empty.into_operation().is_err());
    }
}
