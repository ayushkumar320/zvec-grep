use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use tokio::sync::Notify;
use zg_engine::{ClockPort, CoreError, RunControl, WorkspaceChangeBatch, WorkspaceWatcherPort};

#[derive(Debug, Default)]
pub struct ManualClock {
    now_epoch_ms: AtomicU64,
}

impl ManualClock {
    pub fn set(&self, value: u64) {
        self.now_epoch_ms.store(value, Ordering::Release);
    }

    pub fn advance(&self, delta_ms: u64) {
        self.now_epoch_ms.fetch_add(delta_ms, Ordering::AcqRel);
    }
}

impl ClockPort for ManualClock {
    fn now_epoch_ms(&self) -> u64 {
        self.now_epoch_ms.load(Ordering::Acquire)
    }
}

#[derive(Debug, Default)]
pub struct ManualWatcher {
    batches: Mutex<VecDeque<WorkspaceChangeBatch>>,
    changed: Notify,
}

impl ManualWatcher {
    pub fn push(&self, batch: WorkspaceChangeBatch) {
        self.lock_batches().push_back(batch);
        self.changed.notify_one();
    }

    fn lock_batches(&self) -> MutexGuard<'_, VecDeque<WorkspaceChangeBatch>> {
        match self.batches.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[async_trait]
impl WorkspaceWatcherPort for ManualWatcher {
    async fn next_changes(
        &self,
        _root: &Path,
        control: &RunControl,
    ) -> Result<WorkspaceChangeBatch, CoreError> {
        loop {
            let notified = self.changed.notified();
            if let Some(batch) = self.lock_batches().pop_front() {
                return Ok(batch);
            }
            tokio::select! {
                () = control.cancellation.cancelled() => return Err(CoreError::Cancelled),
                () = notified => {}
            }
        }
    }
}
