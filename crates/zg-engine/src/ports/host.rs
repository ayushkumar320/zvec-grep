use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;

use crate::{CoreError, FileKind, RootSpec, RunControl, SkippedFile, WorkspaceChangeBatch};

pub trait ClockPort: Send + Sync {
    fn now_epoch_ms(&self) -> u64;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanRequest {
    pub roots: Vec<RootSpec>,
    /// Previously indexed source fingerprints keyed by root and relative path.
    /// A scanner may reuse matching metadata without repeating binary sniffing.
    pub known_files: Vec<KnownSourceFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownSourceFile {
    pub root: PathBuf,
    pub relative_path: PathBuf,
    pub source_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredFile {
    pub root: PathBuf,
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub modified_epoch_ms: Option<u64>,
    pub source_fingerprint: String,
    pub kind_hint: Option<FileKind>,
    pub format_hint: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SkippedByReason {
    pub empty: usize,
    pub too_large: usize,
    pub unsupported: usize,
    pub binary: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanDiagnostics {
    pub skipped_files: usize,
    pub skipped_by_reason: SkippedByReason,
    /// Bounded diagnostic samples; production scanners retain at most 20.
    pub skipped_samples: Vec<SkippedFile>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanSnapshot {
    pub files: Vec<DiscoveredFile>,
    pub diagnostics: ScanDiagnostics,
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
    pub format_hint: Option<String>,
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
    pub root: RootSpec,
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
/// Native file rename events are normalized into Delete plus Upsert. Directory
/// changes retain their scope through `RescanDirectory` or `DeletePrefix`.
/// Native queue overflow and watcher recovery are normalized into one Rescan.
#[async_trait]
pub trait WorkspaceWatchSessionPort: Send + Sync {
    async fn next_changes(&self, control: &RunControl) -> Result<WorkspaceChangeBatch, CoreError>;

    async fn close(&self) -> Result<(), CoreError>;
}
