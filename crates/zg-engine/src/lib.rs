//! High-level Rust API for zvec-grep.
//!
//! Create one [`ZvecGrep`] for a process and call its typed methods. Command
//! dispatch, transport envelopes and adapter composition are not part of this
//! public API.

pub mod api;
mod error;
#[allow(dead_code)]
mod extraction;
mod indexing;
mod lexical;
#[allow(dead_code)]
mod models;
mod payload;
mod service;
mod storage;
mod workspace;

use api::{
    context::{ContextOptions, ContextResult},
    index::{IndexOptions, IndexResult},
    info::{InfoOptions, InfoResult},
};

pub use error::{EngineError, ErrorCode};

/// Reusable zvec-grep engine. One instance may serve multiple workspaces.
#[derive(Clone, Debug)]
pub struct ZvecGrep {
    service: service::EngineService,
}

#[allow(clippy::missing_errors_doc)]
impl ZvecGrep {
    #[must_use]
    pub fn new() -> Self {
        Self {
            service: service::EngineService::new(),
        }
    }

    pub async fn context(&self, options: ContextOptions) -> Result<ContextResult, EngineError> {
        self.service.context(options).await
    }

    pub async fn index(&self, options: IndexOptions) -> Result<IndexResult, EngineError> {
        self.service.index(options).await
    }

    pub async fn info(&self, options: InfoOptions) -> Result<InfoResult, EngineError> {
        self.service.info(options).await
    }

    pub async fn drop_index(&self, options: InfoOptions) -> Result<bool, EngineError> {
        self.service.drop_index(options).await
    }

    pub async fn disable_index(&self, options: InfoOptions) -> Result<InfoResult, EngineError> {
        self.service.disable_index(options).await
    }

    pub fn close(&self) {
        self.service.close();
    }
}

impl Default for ZvecGrep {
    fn default() -> Self {
        Self::new()
    }
}
