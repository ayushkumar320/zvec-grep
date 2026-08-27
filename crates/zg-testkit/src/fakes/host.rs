use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use tokio::sync::Notify;
use zg_engine::{
    ClockPort, CoreError, DiscoveredFile, ReadBatchRequest, RunControl, ScanDiagnostics,
    ScanRequest, ScanSnapshot, SourceFile, WatchRequest, WorkspaceChangeBatch,
    WorkspaceScannerPort, WorkspaceWatchSessionPort, WorkspaceWatcherFactoryPort,
};

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

#[derive(Clone, Debug, Default)]
pub struct FixtureScanner {
    files: Arc<Mutex<BTreeMap<(PathBuf, PathBuf), SourceFile>>>,
}

impl FixtureScanner {
    pub fn insert(&self, file: SourceFile) {
        let key = (file.root.clone(), file.relative_path.clone());
        lock(&self.files).insert(key, file);
    }
}

#[async_trait]
impl WorkspaceScannerPort for FixtureScanner {
    async fn discover(
        &self,
        request: &ScanRequest,
        control: &RunControl,
    ) -> Result<ScanSnapshot, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let roots: Vec<_> = request.roots.iter().map(|root| &root.path).collect();
        let files = lock(&self.files)
            .values()
            .filter(|file| roots.is_empty() || roots.contains(&&file.root))
            .map(|file| DiscoveredFile {
                root: file.root.clone(),
                relative_path: file.relative_path.clone(),
                size_bytes: file.bytes.len() as u64,
                modified_epoch_ms: None,
                source_fingerprint: file.source_fingerprint.clone(),
                kind_hint: file.kind_hint,
                format_hint: file.format_hint.clone(),
            })
            .collect();
        Ok(ScanSnapshot {
            files,
            diagnostics: ScanDiagnostics::default(),
        })
    }

    async fn read_batch(
        &self,
        request: &ReadBatchRequest,
        control: &RunControl,
    ) -> Result<Vec<SourceFile>, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let files = lock(&self.files);
        request
            .files
            .iter()
            .map(|file| {
                files
                    .get(&(file.root.clone(), file.relative_path.clone()))
                    .cloned()
                    .ok_or_else(|| {
                        CoreError::invalid_input(format!(
                            "fixture source is missing: {}",
                            file.relative_path.display()
                        ))
                    })
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ManualWatcher {
    inner: Arc<ManualWatcherInner>,
}

#[derive(Debug, Default)]
struct ManualWatcherInner {
    batches: Mutex<VecDeque<WorkspaceChangeBatch>>,
    requests: Mutex<Vec<WatchRequest>>,
    changed: Notify,
    closed: AtomicBool,
}

impl ManualWatcher {
    pub fn push(&self, batch: WorkspaceChangeBatch) {
        lock(&self.inner.batches).push_back(batch);
        self.inner.changed.notify_one();
    }

    #[must_use]
    pub fn requests(&self) -> Vec<WatchRequest> {
        lock(&self.inner.requests).clone()
    }
}

#[async_trait]
impl WorkspaceWatcherFactoryPort for ManualWatcher {
    async fn watch(
        &self,
        request: &WatchRequest,
        control: &RunControl,
    ) -> Result<Arc<dyn WorkspaceWatchSessionPort>, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        self.inner.closed.store(false, Ordering::Release);
        lock(&self.inner.requests).push(request.clone());
        Ok(Arc::new(self.clone()))
    }
}

#[async_trait]
impl WorkspaceWatchSessionPort for ManualWatcher {
    async fn next_changes(&self, control: &RunControl) -> Result<WorkspaceChangeBatch, CoreError> {
        loop {
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(CoreError::ShuttingDown);
            }
            let notified = self.inner.changed.notified();
            if let Some(batch) = lock(&self.inner.batches).pop_front() {
                return Ok(batch);
            }
            tokio::select! {
                () = control.cancellation.cancelled() => return Err(CoreError::Cancelled),
                () = notified => {}
            }
        }
    }

    async fn close(&self) -> Result<(), CoreError> {
        self.inner.closed.store(true, Ordering::Release);
        self.inner.changed.notify_waiters();
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
