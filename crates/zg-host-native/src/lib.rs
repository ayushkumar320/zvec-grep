//! Native filesystem scanner and watcher adapters.

mod api;
mod change_set;
mod file_type;
mod pattern;
mod policy;
mod scanner;
mod watcher;

pub use api::{
    ClockPort, DiscoveredFile, DiscoveryOptions, FileKind, KnownSourceFile, ReadBatchRequest,
    RootSpec, ScanDiagnostics, ScanRequest, ScanSnapshot, SkippedByReason, SkippedFile,
    SkippedFileReason, SourceFile, TaskControl, WatchRequest, WorkspaceChange,
    WorkspaceChangeBatch, WorkspaceScannerPort, WorkspaceWatchSessionPort,
    WorkspaceWatcherFactoryPort,
};
pub use scanner::NativeScanner;
pub use watcher::{NativeWatcherConfig, NativeWatcherFactory};
