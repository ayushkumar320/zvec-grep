use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;

use crate::{CoreError, DiscoveryOptions, FileKind, RootSpec, RunControl, SkippedFile};

pub trait ClockPort: Send + Sync {
    fn now_epoch_ms(&self) -> u64;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanRequest {
    pub roots: Vec<RootSpec>,
    pub discovery: DiscoveryOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredFile {
    pub root: PathBuf,
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub modified_epoch_ms: Option<u64>,
    pub source_fingerprint: String,
    pub kind_hint: Option<FileKind>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanSnapshot {
    pub files: Vec<DiscoveredFile>,
    pub skipped: Vec<SkippedFile>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReadBatchRequest {
    pub files: Vec<DiscoveredFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    pub root: PathBuf,
    pub relative_path: PathBuf,
    pub bytes: Vec<u8>,
    pub source_fingerprint: String,
    pub kind_hint: Option<FileKind>,
}

/// Metadata-first workspace discovery and bounded source reads.
///
/// Discovery must not read complete file contents. Callers compare the source
/// fingerprints with indexed file state, then request bytes only for files
/// that need extraction.
#[async_trait]
pub trait WorkspaceScannerPort: Send + Sync {
    async fn discover(
        &self,
        request: &ScanRequest,
        control: &RunControl,
    ) -> Result<ScanSnapshot, CoreError>;

    async fn read_batch(
        &self,
        request: &ReadBatchRequest,
        control: &RunControl,
    ) -> Result<Vec<SourceFile>, CoreError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchRequest {
    pub root: PathBuf,
    pub discovery: DiscoveryOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceChange {
    Upsert(PathBuf),
    Delete(PathBuf),
    Rescan,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceChangeBatch {
    pub changes: Vec<WorkspaceChange>,
}

/// Creates one resident watch session for a workspace root.
#[async_trait]
pub trait WorkspaceWatcherFactoryPort: Send + Sync {
    async fn watch(
        &self,
        request: &WatchRequest,
        control: &RunControl,
    ) -> Result<Arc<dyn WorkspaceWatchSessionPort>, CoreError>;
}

/// A daemon-owned watch session.
///
/// Native rename events are normalized into Delete plus Upsert. Native queue
/// overflow is normalized into a single Rescan change.
#[async_trait]
pub trait WorkspaceWatchSessionPort: Send + Sync {
    async fn next_changes(&self, control: &RunControl) -> Result<WorkspaceChangeBatch, CoreError>;

    async fn close(&self) -> Result<(), CoreError>;
}
