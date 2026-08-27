use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use notify::{
    Config as NotifyConfig, Event, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode,
    Watcher, event::RemoveKind,
};
use tokio::{
    sync::{Mutex, Notify, mpsc},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use zg_engine::EngineError;

use crate::{
    api::{
        TaskControl, WatchRequest, WorkspaceChange, WorkspaceChangeBatch,
        WorkspaceWatchSessionPort, WorkspaceWatcherFactoryPort,
    },
    change_set::ChangeSet,
    pattern::normalize_relative_path,
    policy::{FileTypeResolver, RootPolicy},
};

const DEFAULT_RAW_EVENT_CAPACITY: usize = 4_096;
const DEFAULT_BATCH_CAPACITY: usize = 16;

#[derive(Clone, Debug)]
pub struct NativeWatcherConfig {
    pub debounce: Duration,
    pub max_wait: Duration,
    pub reconcile_interval: Option<Duration>,
    pub resume_check_interval: Option<Duration>,
    pub resume_threshold: Duration,
    pub max_changed_paths: usize,
    pub raw_event_capacity: usize,
    pub batch_capacity: usize,
    /// Uses notify's polling backend when set; `None` selects the native backend.
    pub poll_interval: Option<Duration>,
}

impl Default for NativeWatcherConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(750),
            max_wait: Duration::from_secs(5),
            reconcile_interval: Some(Duration::from_hours(1)),
            resume_check_interval: Some(Duration::from_secs(30)),
            resume_threshold: Duration::from_secs(90),
            max_changed_paths: 1_000,
            raw_event_capacity: DEFAULT_RAW_EVENT_CAPACITY,
            batch_capacity: DEFAULT_BATCH_CAPACITY,
            poll_interval: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NativeWatcherFactory {
    resolver: FileTypeResolver,
    config: NativeWatcherConfig,
}

impl NativeWatcherFactory {
    #[must_use]
    pub fn new(ripgrep_executable: impl Into<PathBuf>) -> Self {
        Self {
            resolver: FileTypeResolver::new(ripgrep_executable.into()),
            config: NativeWatcherConfig::default(),
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: NativeWatcherConfig) -> Self {
        self.config = config;
        self
    }
}

impl Default for NativeWatcherFactory {
    fn default() -> Self {
        Self::new("rg")
    }
}

#[async_trait]
impl WorkspaceWatcherFactoryPort for NativeWatcherFactory {
    async fn watch(
        &self,
        request: &WatchRequest,
        control: &TaskControl,
    ) -> Result<Arc<dyn WorkspaceWatchSessionPort>, EngineError> {
        check_control(control)?;
        let root = request.root.clone();
        let resolver = self.resolver.clone();
        let policy_task = tokio::task::spawn_blocking(move || RootPolicy::new(root, &resolver));
        let policy = tokio::select! {
            () = control.cancellation.cancelled() => return Err(EngineError::Cancelled),
            () = deadline_wait(control.deadline) => return Err(EngineError::DeadlineExceeded),
            result = policy_task => result
                .map_err(|error| EngineError::Internal { message: format!("watch policy worker failed: {error}") })??,
        };
        let metadata = std::fs::metadata(policy.root_path()).map_err(|error| {
            EngineError::invalid_input(format!(
                "watch root {} could not be inspected: {error}",
                policy.root_path().display()
            ))
        })?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(EngineError::invalid_input(format!(
                "watch root {} must be a file or directory",
                policy.root_path().display()
            )));
        }

        let config = normalize_config(self.config.clone());
        let (raw_sender, raw_receiver) = mpsc::channel(config.raw_event_capacity);
        let overflowed = Arc::new(AtomicBool::new(false));
        let overflow_notify = Arc::new(Notify::new());
        let watcher = create_watcher(
            policy.root_path(),
            policy.root().recursive,
            &raw_sender,
            &overflowed,
            &overflow_notify,
            config.poll_interval,
        )?;
        let (batch_sender, batch_receiver) = mpsc::channel(config.batch_capacity);
        let close = CancellationToken::new();
        let task = tokio::spawn(watch_loop(WatchLoop {
            policy,
            root_is_file: metadata.is_file(),
            config,
            watcher: Some(watcher),
            raw_sender,
            raw_receiver,
            overflowed,
            overflow_notify,
            batch_sender,
            close: close.clone(),
        }));
        Ok(Arc::new(NativeWatchSession {
            inner: Arc::new(WatchSessionInner {
                receiver: Mutex::new(batch_receiver),
                close,
                task: StdMutex::new(Some(task)),
            }),
        }))
    }
}

#[derive(Debug)]
struct NativeWatchSession {
    inner: Arc<WatchSessionInner>,
}

#[derive(Debug)]
struct WatchSessionInner {
    receiver: Mutex<mpsc::Receiver<WorkspaceChangeBatch>>,
    close: CancellationToken,
    task: StdMutex<Option<JoinHandle<()>>>,
}

impl Drop for WatchSessionInner {
    fn drop(&mut self) {
        self.close.cancel();
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

#[async_trait]
impl WorkspaceWatchSessionPort for NativeWatchSession {
    async fn next_changes(
        &self,
        control: &TaskControl,
    ) -> Result<WorkspaceChangeBatch, EngineError> {
        check_control(control)?;
        if self.inner.close.is_cancelled() {
            return Err(EngineError::Closed);
        }
        let mut receiver = self.inner.receiver.lock().await;
        tokio::select! {
            () = self.inner.close.cancelled() => Err(EngineError::Closed),
            () = control.cancellation.cancelled() => Err(EngineError::Cancelled),
            () = deadline_wait(control.deadline) => Err(EngineError::DeadlineExceeded),
            batch = receiver.recv() => batch.ok_or(EngineError::Closed),
        }
    }

    async fn close(&self) -> Result<(), EngineError> {
        self.inner.close.cancel();
        let task = lock_task(&self.inner.task).take();
        if let Some(task) = task {
            task.await.map_err(|error| EngineError::Internal {
                message: format!("native watcher task failed: {error}"),
            })?;
        }
        Ok(())
    }
}

struct WatchLoop {
    policy: RootPolicy,
    root_is_file: bool,
    config: NativeWatcherConfig,
    watcher: Option<NativeWatcher>,
    raw_sender: mpsc::Sender<notify::Result<Event>>,
    raw_receiver: mpsc::Receiver<notify::Result<Event>>,
    overflowed: Arc<AtomicBool>,
    overflow_notify: Arc<Notify>,
    batch_sender: mpsc::Sender<WorkspaceChangeBatch>,
    close: CancellationToken,
}

#[allow(clippy::too_many_lines)]
async fn watch_loop(mut state: WatchLoop) {
    let mut changes = ChangeSet::new(state.config.max_changed_paths);
    let mut debounce_deadline = None;
    let mut max_wait_deadline = None;
    let mut reconcile_deadline = state
        .config
        .reconcile_interval
        .map(|interval| Instant::now() + interval);
    let mut resume_deadline = state
        .config
        .resume_check_interval
        .map(|interval| Instant::now() + interval);
    let mut last_resume_check = Instant::now();
    let mut retry_deadline = None;
    let mut stable_deadline = Some(Instant::now() + Duration::from_secs(1));
    let mut consecutive_errors = 0_u32;
    let mut recovery_reconcile_pending = false;

    loop {
        tokio::select! {
            () = state.close.cancelled() => break,
            () = sleep_until_option(debounce_deadline) => {
                if !flush_changes(&mut changes, &state.batch_sender, &state.close).await {
                    break;
                }
                debounce_deadline = None;
                max_wait_deadline = None;
            }
            () = sleep_until_option(max_wait_deadline) => {
                if !flush_changes(&mut changes, &state.batch_sender, &state.close).await {
                    break;
                }
                debounce_deadline = None;
                max_wait_deadline = None;
            }
            () = sleep_until_option(reconcile_deadline) => {
                changes.require_full_rescan();
                schedule_flush(&state.config, &mut debounce_deadline, &mut max_wait_deadline);
                reconcile_deadline = state.config.reconcile_interval.map(|interval| Instant::now() + interval);
            }
            () = sleep_until_option(resume_deadline) => {
                let now = Instant::now();
                if now.duration_since(last_resume_check) > state.config.resume_threshold {
                    changes.require_full_rescan();
                    schedule_flush(&state.config, &mut debounce_deadline, &mut max_wait_deadline);
                }
                last_resume_check = now;
                resume_deadline = state.config.resume_check_interval.map(|interval| now + interval);
            }
            () = sleep_until_option(stable_deadline) => {
                consecutive_errors = 0;
                recovery_reconcile_pending = false;
                stable_deadline = None;
            }
            () = sleep_until_option(retry_deadline) => {
                match create_watcher(
                    state.policy.root_path(),
                    state.policy.root().recursive,
                    &state.raw_sender,
                    &state.overflowed,
                    &state.overflow_notify,
                    state.config.poll_interval,
                ) {
                    Ok(watcher) => {
                        state.watcher = Some(watcher);
                        retry_deadline = None;
                        stable_deadline = Some(Instant::now() + Duration::from_secs(1));
                    }
                    Err(error) => {
                        warn!(%error, "failed to recover native watcher");
                        consecutive_errors = consecutive_errors.saturating_add(1);
                        retry_deadline = Some(Instant::now() + retry_delay(consecutive_errors));
                    }
                }
            }
            () = state.overflow_notify.notified() => {
                if state.overflowed.swap(false, Ordering::AcqRel) {
                    changes.require_full_rescan();
                    schedule_flush(&state.config, &mut debounce_deadline, &mut max_wait_deadline);
                }
            }
            raw = state.raw_receiver.recv() => {
                let Some(raw) = raw else { break; };
                match raw {
                    Ok(event) => {
                        let policy = state.policy.clone();
                        let root_is_file = state.root_is_file;
                        match tokio::task::spawn_blocking(move || normalize_event(&policy, root_is_file, &event)).await {
                            Ok(event_changes) => {
                                for change in event_changes {
                                    changes.add(change);
                                }
                                if !changes.is_empty() {
                                    schedule_flush(&state.config, &mut debounce_deadline, &mut max_wait_deadline);
                                }
                            }
                            Err(error) => {
                                warn!(%error, "native watcher event worker failed");
                                changes.require_full_rescan();
                                schedule_flush(&state.config, &mut debounce_deadline, &mut max_wait_deadline);
                            }
                        }
                    }
                    Err(error) => {
                        warn!(%error, "native watcher backend failed");
                        state.watcher.take();
                        consecutive_errors = consecutive_errors.saturating_add(1);
                        stable_deadline = None;
                        retry_deadline = Some(Instant::now() + retry_delay(consecutive_errors));
                        if !recovery_reconcile_pending {
                            recovery_reconcile_pending = true;
                            changes.require_full_rescan();
                            schedule_flush(&state.config, &mut debounce_deadline, &mut max_wait_deadline);
                        }
                    }
                }
            }
        }
    }
    state.watcher.take();
}

fn create_watcher(
    root: &Path,
    recursive: bool,
    raw_sender: &mpsc::Sender<notify::Result<Event>>,
    overflowed: &Arc<AtomicBool>,
    overflow_notify: &Arc<Notify>,
    poll_interval: Option<Duration>,
) -> Result<NativeWatcher, EngineError> {
    let sender = raw_sender.clone();
    let overflowed = Arc::clone(overflowed);
    let overflow_notify = Arc::clone(overflow_notify);
    let handler = move |event| {
        if sender.try_send(event).is_err() {
            overflowed.store(true, Ordering::Release);
            overflow_notify.notify_one();
        }
    };
    let recursive_mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    let watch_error = |error| {
        EngineError::backend(
            "native-watcher",
            format!("could not watch {}: {error}", root.display()),
        )
    };

    if let Some(interval) = poll_interval {
        let mut watcher = PollWatcher::new(
            handler,
            NotifyConfig::default()
                .with_poll_interval(interval)
                .with_compare_contents(true),
        )
        .map_err(|error| EngineError::backend("native-watcher", error.to_string()))?;
        watcher.watch(root, recursive_mode).map_err(watch_error)?;
        Ok(NativeWatcher::Poll { _watcher: watcher })
    } else {
        let mut watcher = notify::recommended_watcher(handler)
            .map_err(|error| EngineError::backend("native-watcher", error.to_string()))?;
        watcher.watch(root, recursive_mode).map_err(watch_error)?;
        Ok(NativeWatcher::Recommended { _watcher: watcher })
    }
}

enum NativeWatcher {
    Recommended { _watcher: RecommendedWatcher },
    Poll { _watcher: PollWatcher },
}

fn normalize_event(policy: &RootPolicy, root_is_file: bool, event: &Event) -> Vec<WorkspaceChange> {
    match event.kind {
        EventKind::Access(_) => Vec::new(),
        EventKind::Any | EventKind::Other => vec![WorkspaceChange::Rescan],
        EventKind::Remove(RemoveKind::File) => event
            .paths
            .iter()
            .filter_map(|path| normalize_removed_path(policy, root_is_file, path, false))
            .collect(),
        EventKind::Remove(_) => event
            .paths
            .iter()
            .filter_map(|path| normalize_removed_path(policy, root_is_file, path, true))
            .collect(),
        EventKind::Create(_) | EventKind::Modify(_) => event
            .paths
            .iter()
            .filter_map(|path| normalize_present_or_removed_path(policy, root_is_file, path))
            .collect(),
    }
}

fn normalize_present_or_removed_path(
    policy: &RootPolicy,
    root_is_file: bool,
    path: &Path,
) -> Option<WorkspaceChange> {
    let metadata = std::fs::metadata(path).ok();
    if metadata.is_none() {
        return normalize_removed_path(policy, root_is_file, path, true);
    }
    normalize_present_path(
        policy,
        root_is_file,
        path,
        metadata.is_some_and(|value| value.is_dir()),
    )
}

fn normalize_present_path(
    policy: &RootPolicy,
    root_is_file: bool,
    path: &Path,
    is_directory: bool,
) -> Option<WorkspaceChange> {
    if path == policy.root_path() && !root_is_file {
        return Some(WorkspaceChange::Rescan);
    }
    let relative = relative_change_path(policy, root_is_file, path)?;
    if path.file_name().is_some_and(|name| name == ".gitignore") {
        return Some(WorkspaceChange::RescanDirectory(parent_scope(&relative)));
    }
    if !policy.path_can_affect_index(path, is_directory) {
        return None;
    }
    Some(if is_directory {
        WorkspaceChange::RescanDirectory(relative)
    } else {
        WorkspaceChange::Upsert(relative)
    })
}

fn normalize_removed_path(
    policy: &RootPolicy,
    root_is_file: bool,
    path: &Path,
    prefix: bool,
) -> Option<WorkspaceChange> {
    if path == policy.root_path() && !root_is_file {
        return Some(WorkspaceChange::Rescan);
    }
    let relative = relative_change_path(policy, root_is_file, path)?;
    if path.file_name().is_some_and(|name| name == ".gitignore") {
        return Some(WorkspaceChange::RescanDirectory(parent_scope(&relative)));
    }
    if !policy.path_can_affect_index(path, false) {
        return None;
    }
    Some(if prefix {
        WorkspaceChange::DeletePrefix(relative)
    } else {
        WorkspaceChange::Delete(relative)
    })
}

fn relative_change_path(policy: &RootPolicy, root_is_file: bool, path: &Path) -> Option<PathBuf> {
    if root_is_file && path == policy.root_path() {
        return policy.root_path().file_name().map(PathBuf::from);
    }
    path.strip_prefix(policy.root_path())
        .ok()
        .and_then(|relative| {
            (!relative.as_os_str().is_empty())
                .then(|| PathBuf::from(normalize_relative_path(relative)))
        })
}

fn parent_scope(path: &Path) -> PathBuf {
    path.parent().unwrap_or_else(|| Path::new("")).to_path_buf()
}

fn schedule_flush(
    config: &NativeWatcherConfig,
    debounce_deadline: &mut Option<Instant>,
    max_wait_deadline: &mut Option<Instant>,
) {
    let now = Instant::now();
    *debounce_deadline = Some(now + config.debounce);
    max_wait_deadline.get_or_insert(now + config.max_wait);
}

async fn flush_changes(
    changes: &mut ChangeSet,
    sender: &mpsc::Sender<WorkspaceChangeBatch>,
    close: &CancellationToken,
) -> bool {
    if changes.is_empty() {
        return true;
    }
    let batch = changes.take_batch();
    tokio::select! {
        () = close.cancelled() => false,
        result = sender.send(batch) => result.is_ok(),
    }
}

async fn sleep_until_option(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        std::future::pending::<()>().await;
    }
}

async fn deadline_wait(deadline: Option<std::time::Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(Instant::from_std(deadline)).await;
    } else {
        std::future::pending::<()>().await;
    }
}

fn retry_delay(consecutive_errors: u32) -> Duration {
    let exponent = consecutive_errors.saturating_sub(1).min(6);
    Duration::from_millis((100_u64.saturating_mul(2_u64.pow(exponent))).min(5_000))
}

fn normalize_config(mut config: NativeWatcherConfig) -> NativeWatcherConfig {
    config.max_changed_paths = config.max_changed_paths.max(1);
    config.raw_event_capacity = config.raw_event_capacity.max(1);
    config.batch_capacity = config.batch_capacity.max(1);
    config
}

fn check_control(control: &TaskControl) -> Result<(), EngineError> {
    if control.cancellation.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    if control
        .deadline
        .is_some_and(|deadline| std::time::Instant::now() >= deadline)
    {
        return Err(EngineError::DeadlineExceeded);
    }
    Ok(())
}

fn lock_task(
    mutex: &StdMutex<Option<JoinHandle<()>>>,
) -> std::sync::MutexGuard<'_, Option<JoinHandle<()>>> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
