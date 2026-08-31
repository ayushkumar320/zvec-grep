use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    EngineError,
    api::{
        context::{
            ContextOptions, ContextResult,
            result::{
                ContentRange, ContextCoverage, ContextDiagnostics, ContextItem, ContextItemKind,
                ContextItemStatus, ContextSource, EmptyReason, MatchedBy, RgDiagnostics,
            },
        },
        index::{IndexOptions, IndexResult},
        info::{InfoOptions, InfoResult},
    },
    indexing::service::WorkspaceIndexService,
    lexical::{
        LexicalSearchService,
        types::{LexicalCoverage, LexicalOptions, LexicalSearchReply, LexicalSearchRequest},
    },
    models::ModelRuntimeManager,
};

const DEFAULT_MAX_CONCURRENT_LEXICAL_SEARCHES: usize = 2;

#[derive(Clone, Debug)]
pub(crate) struct EngineService {
    lexical: LexicalSearchService,
    indexing: WorkspaceIndexService,
    models: ModelRuntimeManager,
    closed: Arc<AtomicBool>,
}

impl EngineService {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            lexical: LexicalSearchService::default()
                .with_max_searches(DEFAULT_MAX_CONCURRENT_LEXICAL_SEARCHES),
            indexing: WorkspaceIndexService::new(),
            models: ModelRuntimeManager::new(),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Retrieves context from an index or, when `rg` is enabled, embedded ripgrep.
    ///
    /// # Errors
    ///
    /// Returns an engine error when the request is invalid or its selected
    /// retrieval mode is unavailable.
    pub(crate) async fn context(
        &self,
        options: ContextOptions,
    ) -> Result<ContextResult, EngineError> {
        self.ensure_open()?;
        let root = resolve_root(options.root.as_deref())?;
        if !options.rg {
            return Err(unavailable("context"));
        }
        if !options.rg_options.extra_args.is_empty() {
            return Err(EngineError::invalid_input(
                "context rg_options.extra_args is not supported by the embedded backend",
            ));
        }
        let query = options
            .query
            .iter()
            .chain(&options.queries)
            .cloned()
            .collect::<Vec<_>>();
        let request = LexicalSearchRequest {
            root: Some(root.clone()),
            patterns: query.clone(),
            pattern_files: options.rg_options.pattern_files,
            paths: options.rg_paths,
            limit: options.limit,
            options: LexicalOptions {
                fixed_strings: options.rg_options.fixed_strings,
                ignore_case: options.rg_options.ignore_case,
                word_regexp: options.rg_options.word_regexp,
                before_context: options.rg_options.before_context,
                after_context: options.rg_options.after_context,
                hidden: options.hidden,
                no_ignore: options.no_ignore,
                follow: options.follow,
                globs: options.globs,
                file_types: options.file_types,
                excluded_file_types: options.excluded_file_types,
                ignore_files: options.ignore_files,
                max_depth: options.max_depth,
                max_file_size_bytes: options.max_file_size_bytes,
                modified_after_epoch_ms: options.modified_after_epoch_ms,
                modified_before_epoch_ms: options.modified_before_epoch_ms,
            },
        };
        let reply = self.lexical.search(&root, &request).await?;
        Ok(context_from_lexical(query.join("\n"), reply))
    }

    /// Creates or refreshes the workspace index.
    ///
    /// # Errors
    ///
    /// Returns an engine error when indexing fails or no storage backend is configured.
    pub(crate) async fn index(&self, options: IndexOptions) -> Result<IndexResult, EngineError> {
        self.ensure_open()?;
        self.indexing.index(&self.models, options).await
    }

    /// Returns workspace index metadata and status.
    ///
    /// # Errors
    ///
    /// Returns an engine error when workspace metadata or status cannot be read.
    pub(crate) async fn info(&self, options: InfoOptions) -> Result<InfoResult, EngineError> {
        self.ensure_open()?;
        self.indexing.info(options).await
    }

    /// Drops the persisted workspace index.
    ///
    /// # Errors
    ///
    /// Returns an engine error when index removal fails or no storage backend is configured.
    pub(crate) async fn drop_index(&self, options: InfoOptions) -> Result<bool, EngineError> {
        self.ensure_open()?;
        std::future::ready(self.indexing.drop_index(&options)).await
    }

    /// Disables indexing for a workspace and returns its updated information.
    ///
    /// # Errors
    ///
    /// Returns an engine error when workspace metadata cannot be updated.
    pub(crate) async fn disable_index(
        &self,
        options: InfoOptions,
    ) -> Result<InfoResult, EngineError> {
        self.ensure_open()?;
        std::future::ready(WorkspaceIndexService::disable_index(&options)).await
    }

    /// Closes this service and rejects subsequent requests.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.models.close();
    }

    fn ensure_open(&self) -> Result<(), EngineError> {
        if self.closed.load(Ordering::Acquire) {
            Err(EngineError::Closed)
        } else {
            Ok(())
        }
    }
}

fn context_from_lexical(query: String, reply: LexicalSearchReply) -> ContextResult {
    let hits_returned = reply.matches.len();
    let empty_reason = (hits_returned == 0).then_some(EmptyReason::NoMatches);
    ContextResult {
        query,
        root: reply.root,
        source: ContextSource::Rg,
        coverage: match reply.coverage {
            LexicalCoverage::Exhaustive => ContextCoverage::RgExhaustive,
            LexicalCoverage::Truncated => ContextCoverage::RgTruncated,
        },
        workspace_index: None,
        items: reply
            .matches
            .into_iter()
            .map(|item| ContextItem {
                kind: ContextItemKind::LexicalMatch,
                rank: item.rank,
                absolute_path: item.absolute_path,
                relative_path: item.relative_path,
                range: lexical_range(item.range),
                excerpt_range: item.excerpt_range.map(lexical_range),
                content: item.content,
                outline: None,
                status: ContextItemStatus::Fresh,
                score: None,
                matched_by: MatchedBy::Lexical,
                metadata: None,
                entity_id: None,
            })
            .collect(),
        diagnostics: ContextDiagnostics {
            empty_reason,
            hits_returned,
            rg: Some(RgDiagnostics {
                backend: reply.diagnostics.backend,
                command: reply.diagnostics.command,
                args: reply.diagnostics.args,
                ignored_directories: reply.diagnostics.ignored_directories,
                missing_paths: reply.diagnostics.missing_paths,
                searched_paths: reply.diagnostics.searched_paths,
                limit: reply.diagnostics.limit,
                truncated: reply.diagnostics.truncated,
            }),
            timings: Vec::new(),
        },
    }
}

fn lexical_range(range: crate::lexical::types::TextRange) -> ContentRange {
    ContentRange::Text {
        start_line: range.start_line,
        end_line: range.end_line,
        start_offset: range.start_offset,
        end_offset: range.end_offset,
    }
}

fn resolve_root(root: Option<&Path>) -> Result<PathBuf, EngineError> {
    std::path::absolute(root.unwrap_or_else(|| Path::new("."))).map_err(|error| {
        EngineError::backend("workspace", format!("failed to resolve root: {error}"))
    })
}

fn unavailable(capability: &str) -> EngineError {
    EngineError::CapabilityUnavailable {
        capability: capability.to_owned(),
    }
}
