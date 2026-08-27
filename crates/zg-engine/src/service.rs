use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    ChangeIndexReply, ChangeIndexRequest, EngineError, IndexReply, IndexRequest, InspectReply,
    InspectRequest, JobReply, JobRequest, LexicalSearchReply, LexicalSearchRequest, QueryReply,
    QueryRequest, lexical::LexicalSearchService, models::runtime::ModelRuntimeManager,
};

const DEFAULT_MAX_CONCURRENT_LEXICAL_SEARCHES: usize = 2;

/// Reusable zvec-grep engine serving workspace roots supplied by each request.
#[derive(Clone, Debug)]
pub struct ZvecGrep {
    lexical: LexicalSearchService,
    models: ModelRuntimeManager,
    closed: Arc<AtomicBool>,
}

#[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
impl ZvecGrep {
    /// Creates a reusable engine instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            lexical: LexicalSearchService::default()
                .with_max_searches(DEFAULT_MAX_CONCURRENT_LEXICAL_SEARCHES),
            models: ModelRuntimeManager::new(),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Runs an indexed or routed context query.
    ///
    /// # Errors
    ///
    /// Returns an engine error when indexed query support is unavailable.
    pub async fn query(&self, request: QueryRequest) -> Result<QueryReply, EngineError> {
        self.ensure_open()?;
        let _root = resolve_root(request.root.as_deref())?;
        Err(unavailable("query"))
    }

    /// Runs an exhaustive lexical search.
    ///
    /// # Errors
    ///
    /// Returns an engine error when the request is invalid or embedded grep fails.
    pub async fn lexical_search(
        &self,
        request: LexicalSearchRequest,
    ) -> Result<LexicalSearchReply, EngineError> {
        self.ensure_open()?;
        let root = resolve_root(request.root.as_deref())?;
        self.lexical.search(&root, &request).await
    }

    /// Creates or refreshes the workspace index.
    ///
    /// # Errors
    ///
    /// Returns an engine error when indexing support is unavailable.
    pub async fn index(&self, request: IndexRequest) -> Result<IndexReply, EngineError> {
        self.ensure_open()?;
        let _root = resolve_root(request.root.as_deref())?;
        Err(unavailable("index"))
    }

    /// Inspects workspace index metadata and status.
    ///
    /// # Errors
    ///
    /// Returns an engine error when inspection support is unavailable.
    pub async fn inspect(&self, request: InspectRequest) -> Result<InspectReply, EngineError> {
        self.ensure_open()?;
        let _root = resolve_root(request.root.as_deref())?;
        Err(unavailable("inspect"))
    }

    /// Drops or disables the workspace index.
    ///
    /// # Errors
    ///
    /// Returns an engine error when index mutation support is unavailable.
    pub async fn change_index(
        &self,
        request: ChangeIndexRequest,
    ) -> Result<ChangeIndexReply, EngineError> {
        self.ensure_open()?;
        let _root = resolve_root(request.root.as_deref())?;
        Err(unavailable("change_index"))
    }

    /// Lists, inspects or cancels background jobs.
    ///
    /// # Errors
    ///
    /// Returns an engine error when job support is unavailable.
    pub async fn job(&self, request: JobRequest) -> Result<JobReply, EngineError> {
        self.ensure_open()?;
        let _root = resolve_root(request.root.as_deref())?;
        Err(unavailable("job"))
    }

    /// Closes this service and rejects subsequent requests.
    pub fn close(&self) {
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

impl Default for ZvecGrep {
    fn default() -> Self {
        Self::new()
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
