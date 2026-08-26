use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::{CoreError, RunControl};

pub trait ClockPort: Send + Sync {
    fn now_epoch_ms(&self) -> u64;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceChangeBatch {
    pub paths: Vec<PathBuf>,
    pub overflowed: bool,
}

/// Resident watcher seam. Each call returns the next coalesced change batch.
#[async_trait]
pub trait WorkspaceWatcherPort: Send + Sync {
    async fn next_changes(
        &self,
        root: &Path,
        control: &RunControl,
    ) -> Result<WorkspaceChangeBatch, CoreError>;
}
