use std::{fmt, sync::Arc, thread};

use crate::{
    ClockPort, CoreError, EmbeddingFactoryPort, ExtractionPort, IndexStoragePort,
    LexicalSearchPort, WorkspaceScannerPort,
};

/// Coarse process-level limits shared by Direct and Server hosts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceBudget {
    pub max_concurrent_operations: usize,
    pub max_cpu_tasks: usize,
    pub max_blocking_tasks: usize,
    pub max_background_jobs: usize,
    pub max_lexical_processes: usize,
}

impl ResourceBudget {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        for (name, value) in [
            ("max_concurrent_operations", self.max_concurrent_operations),
            ("max_cpu_tasks", self.max_cpu_tasks),
            ("max_blocking_tasks", self.max_blocking_tasks),
            ("max_lexical_processes", self.max_lexical_processes),
        ] {
            if value == 0 {
                return Err(CoreError::invalid_input(format!(
                    "resource budget {name} must be greater than zero"
                )));
            }
        }
        Ok(())
    }
}

impl Default for ResourceBudget {
    fn default() -> Self {
        let processors = thread::available_parallelism().map_or(1, usize::from);
        Self {
            max_concurrent_operations: processors.max(2),
            max_cpu_tasks: processors,
            max_blocking_tasks: processors.saturating_mul(2).max(2),
            max_background_jobs: 1,
            max_lexical_processes: 2,
        }
    }
}

/// Internal production or test adapters registered by a composition root.
#[derive(Clone, Default)]
pub struct CorePorts {
    lexical: Option<Arc<dyn LexicalSearchPort>>,
    scanner: Option<Arc<dyn WorkspaceScannerPort>>,
    extraction: Option<Arc<dyn ExtractionPort>>,
    storage: Option<Arc<dyn IndexStoragePort>>,
    embedding: Option<Arc<dyn EmbeddingFactoryPort>>,
    clock: Option<Arc<dyn ClockPort>>,
}

impl CorePorts {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_lexical(mut self, port: Arc<dyn LexicalSearchPort>) -> Self {
        self.lexical = Some(port);
        self
    }

    #[must_use]
    pub fn with_scanner(mut self, port: Arc<dyn WorkspaceScannerPort>) -> Self {
        self.scanner = Some(port);
        self
    }

    #[must_use]
    pub fn with_extraction(mut self, port: Arc<dyn ExtractionPort>) -> Self {
        self.extraction = Some(port);
        self
    }

    #[must_use]
    pub fn with_storage(mut self, port: Arc<dyn IndexStoragePort>) -> Self {
        self.storage = Some(port);
        self
    }

    #[must_use]
    pub fn with_embedding(mut self, port: Arc<dyn EmbeddingFactoryPort>) -> Self {
        self.embedding = Some(port);
        self
    }

    #[must_use]
    pub fn with_clock(mut self, port: Arc<dyn ClockPort>) -> Self {
        self.clock = Some(port);
        self
    }

    #[must_use]
    pub fn capabilities(&self) -> Vec<&'static str> {
        let mut capabilities = Vec::new();
        if self.lexical.is_some() {
            capabilities.push("lexical_search");
        }
        if self.scanner.is_some() {
            capabilities.push("scanner");
        }
        if self.extraction.is_some() {
            capabilities.push("extraction");
        }
        if self.storage.is_some() {
            capabilities.push("storage");
        }
        if self.embedding.is_some() {
            capabilities.push("embedding");
        }
        if self.clock.is_some() {
            capabilities.push("clock");
        }
        capabilities
    }

    pub(crate) fn lexical(&self) -> Result<&Arc<dyn LexicalSearchPort>, CoreError> {
        self.lexical
            .as_ref()
            .ok_or_else(|| CoreError::CapabilityUnavailable {
                capability: "lexical_search".to_owned(),
            })
    }
}

impl fmt::Debug for CorePorts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CorePorts")
            .field("capabilities", &self.capabilities())
            .finish()
    }
}

/// Everything required to open a Core. Native types remain behind `CorePorts`.
#[derive(Clone, Debug)]
pub struct CoreConfig {
    pub resources: ResourceBudget,
    pub ports: CorePorts,
}

impl CoreConfig {
    #[must_use]
    pub fn new(ports: CorePorts) -> Self {
        Self {
            resources: ResourceBudget::default(),
            ports,
        }
    }

    #[must_use]
    pub fn with_resources(mut self, resources: ResourceBudget) -> Self {
        self.resources = resources;
        self
    }
}
