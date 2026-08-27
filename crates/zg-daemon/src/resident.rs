use std::{
    collections::HashMap,
    path::Path,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use thiserror::Error;
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use zg_engine::{
    ChangeIndexAction, Command, CoreError, ErrorReply, IndexRequest, Operation, OperationExecutor,
    Outcome, Reply, RootSpec, RunControl, WatchRequest, WorkspaceChangeBatch,
    WorkspaceWatchSessionPort, WorkspaceWatcherFactoryPort,
};

/// Owns daemon-resident watcher sessions and their incremental index loops.
#[derive(Clone)]
pub struct ResidentWorkspaceManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    executor: Arc<dyn OperationExecutor>,
    watcher_factory: Arc<dyn WorkspaceWatcherFactoryPort>,
    sessions: Mutex<HashMap<PathBuf, ResidentWorkspace>>,
    lifecycle: Mutex<()>,
    shutdown: CancellationToken,
    active_count: AtomicUsize,
}

struct ResidentWorkspace {
    root: RootSpec,
    session: Arc<dyn WorkspaceWatchSessionPort>,
    cancellation: CancellationToken,
    task: JoinHandle<Result<(), ResidentWorkspaceError>>,
}

/// Failure while creating, driving, or stopping a resident workspace watcher.
#[derive(Debug, Error)]
pub enum ResidentWorkspaceError {
    #[error("resident workspace root must be absolute: {0}")]
    RelativeRoot(PathBuf),
    #[error("resident workspace manager is shutting down")]
    ShuttingDown,
    #[error("failed to create watcher for {root}: {source}")]
    StartWatcher {
        root: PathBuf,
        #[source]
        source: CoreError,
    },
    #[error("watcher for {root} failed: {source}")]
    Watch {
        root: PathBuf,
        #[source]
        source: CoreError,
    },
    #[error("failed to close watcher for {root}: {source}")]
    CloseWatcher {
        root: PathBuf,
        #[source]
        source: CoreError,
    },
    #[error("incremental index for {root} failed: {message}")]
    Index { root: PathBuf, message: String },
    #[error("incremental index for {root} requested authorization: {reason}")]
    AuthorizationRequired { root: PathBuf, reason: String },
    #[error("incremental index for {root} returned a non-index reply")]
    UnexpectedReply { root: PathBuf },
    #[error("resident task for {root} failed to join: {message}")]
    Join { root: PathBuf, message: String },
}

impl ResidentWorkspaceManager {
    #[must_use]
    pub fn new(
        watcher_factory: Arc<dyn WorkspaceWatcherFactoryPort>,
        executor: Arc<dyn OperationExecutor>,
    ) -> Self {
        Self {
            inner: Arc::new(ManagerInner {
                executor,
                watcher_factory,
                sessions: Mutex::new(HashMap::new()),
                lifecycle: Mutex::new(()),
                shutdown: CancellationToken::new(),
                active_count: AtomicUsize::new(0),
            }),
        }
    }

    /// Ensures that exactly one live watcher loop exists for `root`.
    ///
    /// Repeating the same request is idempotent. A changed `RootSpec` replaces
    /// the old session so scanner and watcher continue to share one policy.
    ///
    /// # Errors
    ///
    /// Returns an error for a relative root, during shutdown, or when the
    /// watcher session cannot be created.
    pub async fn ensure_watching(&self, root: RootSpec) -> Result<(), ResidentWorkspaceError> {
        if !root.path.is_absolute() {
            return Err(ResidentWorkspaceError::RelativeRoot(root.path));
        }

        let _lifecycle = self.inner.lifecycle.lock().await;
        if self.inner.shutdown.is_cancelled() {
            return Err(ResidentWorkspaceError::ShuttingDown);
        }

        let key = root.path.clone();
        let previous = self.inner.sessions.lock().await.remove(&key);
        if let Some(previous) = previous {
            if previous.root == root && !previous.task.is_finished() {
                self.inner.sessions.lock().await.insert(key, previous);
                return Ok(());
            }
            self.inner.active_count.fetch_sub(1, Ordering::AcqRel);
            if let Err(error) = stop_workspace(previous).await {
                warn!(%error, "failed to retire resident workspace session before restart");
            }
        }

        let cancellation = self.inner.shutdown.child_token();
        let control = RunControl::local(cancellation.clone());
        let session = self
            .inner
            .watcher_factory
            .watch(&WatchRequest { root: root.clone() }, &control)
            .await
            .map_err(|source| ResidentWorkspaceError::StartWatcher {
                root: key.clone(),
                source,
            })?;
        let task = tokio::spawn(drive_workspace(
            root.clone(),
            Arc::clone(&session),
            Arc::clone(&self.inner.executor),
            cancellation.clone(),
        ));
        self.inner.sessions.lock().await.insert(
            key,
            ResidentWorkspace {
                root,
                session,
                cancellation,
                task,
            },
        );
        self.inner.active_count.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    #[must_use]
    pub fn active_count(&self) -> usize {
        self.inner.active_count.load(Ordering::Acquire)
    }

    /// Stops and removes the watcher for `root`.
    ///
    /// Returns `true` when a resident session existed.
    ///
    /// # Errors
    ///
    /// Returns an error if the watcher cannot close or its task cannot join.
    pub async fn stop_watching(&self, root: &Path) -> Result<bool, ResidentWorkspaceError> {
        let _lifecycle = self.inner.lifecycle.lock().await;
        let workspace = self.inner.sessions.lock().await.remove(root);
        let Some(workspace) = workspace else {
            return Ok(false);
        };
        self.inner.active_count.fetch_sub(1, Ordering::AcqRel);
        stop_workspace(workspace).await?;
        Ok(true)
    }

    /// Stops every watcher and permanently closes this manager.
    ///
    /// All sessions are given a chance to stop even if an earlier one fails.
    ///
    /// # Errors
    ///
    /// Returns the first close or task failure after attempting all sessions.
    pub async fn shutdown_all(&self) -> Result<(), ResidentWorkspaceError> {
        let _lifecycle = self.inner.lifecycle.lock().await;
        self.inner.shutdown.cancel();
        let workspaces = self
            .inner
            .sessions
            .lock()
            .await
            .drain()
            .map(|(_, workspace)| workspace)
            .collect::<Vec<_>>();
        self.inner.active_count.store(0, Ordering::Release);
        let mut first_error = None;
        for workspace in workspaces {
            if let Err(error) = stop_workspace(workspace).await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for ManagerInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

/// Daemon-only execution decorator that attaches or detaches resident watchers
/// after a synchronous index lifecycle operation has completed successfully.
pub(crate) struct ResidentOperationExecutor {
    delegate: Arc<dyn OperationExecutor>,
    residents: ResidentWorkspaceManager,
}

impl ResidentOperationExecutor {
    pub(crate) const fn new(
        delegate: Arc<dyn OperationExecutor>,
        residents: ResidentWorkspaceManager,
    ) -> Self {
        Self {
            delegate,
            residents,
        }
    }
}

#[async_trait]
impl OperationExecutor for ResidentOperationExecutor {
    async fn execute(
        &self,
        operation: Operation,
        control: RunControl,
    ) -> Result<Outcome, ErrorReply> {
        let follow_up = ResidentFollowUp::from_operation(&operation);
        let outcome = self.delegate.execute(operation, control).await?;
        if follow_up.applies_to(&outcome) {
            follow_up.apply(&self.residents).await.map_err(|error| {
                CoreError::backend("resident-workspace-manager", error.to_string()).to_reply()
            })?;
        }
        Ok(outcome)
    }
}

enum ResidentFollowUp {
    Ensure(Vec<RootSpec>),
    Stop(PathBuf),
    None,
}

impl ResidentFollowUp {
    fn from_operation(operation: &Operation) -> Self {
        match &operation.command {
            Command::Index(request) => Self::Ensure(request.roots.clone()),
            Command::ChangeIndex(request)
                if matches!(
                    request.action,
                    ChangeIndexAction::Drop | ChangeIndexAction::Disable
                ) =>
            {
                Self::Stop(operation.root.clone())
            }
            _ => Self::None,
        }
    }

    fn applies_to(&self, outcome: &Outcome) -> bool {
        matches!(
            (self, outcome),
            (Self::Ensure(_), Outcome::Completed(reply)) if matches!(reply.as_ref(), Reply::Index(_))
        ) || matches!(
            (self, outcome),
            (Self::Stop(_), Outcome::Completed(reply)) if matches!(reply.as_ref(), Reply::ChangeIndex(_))
        )
    }

    async fn apply(
        &self,
        residents: &ResidentWorkspaceManager,
    ) -> Result<(), ResidentWorkspaceError> {
        match self {
            Self::Ensure(roots) => {
                for root in roots {
                    residents.ensure_watching(root.clone()).await?;
                }
                Ok(())
            }
            Self::Stop(root) => residents.stop_watching(root).await.map(|_| ()),
            Self::None => Ok(()),
        }
    }
}

async fn drive_workspace(
    root: RootSpec,
    session: Arc<dyn WorkspaceWatchSessionPort>,
    executor: Arc<dyn OperationExecutor>,
    cancellation: CancellationToken,
) -> Result<(), ResidentWorkspaceError> {
    let result =
        drive_workspace_until_stopped(&root, session.as_ref(), executor.as_ref(), &cancellation)
            .await;
    let close_result =
        session
            .close()
            .await
            .map_err(|source| ResidentWorkspaceError::CloseWatcher {
                root: root.path.clone(),
                source,
            });
    if let Err(error) = &result {
        warn!(root = %root.path.display(), %error, "resident workspace loop stopped");
    }
    result.and(close_result)
}

async fn drive_workspace_until_stopped(
    root: &RootSpec,
    session: &dyn WorkspaceWatchSessionPort,
    executor: &dyn OperationExecutor,
    cancellation: &CancellationToken,
) -> Result<(), ResidentWorkspaceError> {
    loop {
        let control = RunControl::local(cancellation.clone());
        let changes = match session.next_changes(&control).await {
            Ok(changes) => changes,
            Err(CoreError::Cancelled | CoreError::ShuttingDown) if cancellation.is_cancelled() => {
                return Ok(());
            }
            Err(source) => {
                return Err(ResidentWorkspaceError::Watch {
                    root: root.path.clone(),
                    source,
                });
            }
        };
        if changes.changes.is_empty() {
            continue;
        }

        let outcome = executor
            .execute(index_operation(root, changes), control)
            .await;
        match outcome {
            Ok(Outcome::Completed(reply)) if matches!(reply.as_ref(), Reply::Index(_)) => {}
            Ok(Outcome::Accepted(_)) => {}
            Ok(Outcome::InputRequired(challenge)) => {
                return Err(ResidentWorkspaceError::AuthorizationRequired {
                    root: root.path.clone(),
                    reason: challenge.reason,
                });
            }
            Ok(Outcome::Completed(_)) => {
                return Err(ResidentWorkspaceError::UnexpectedReply {
                    root: root.path.clone(),
                });
            }
            Err(error)
                if cancellation.is_cancelled()
                    && matches!(
                        error.code,
                        zg_engine::ErrorCode::Cancelled | zg_engine::ErrorCode::ShuttingDown
                    ) =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(ResidentWorkspaceError::Index {
                    root: root.path.clone(),
                    message: error.message,
                });
            }
        }
    }
}

fn index_operation(root: &RootSpec, changes: WorkspaceChangeBatch) -> Operation {
    Operation::new(
        root.path.clone(),
        Command::Index(IndexRequest {
            roots: vec![root.clone()],
            changes: changes.changes,
            discovery: root.discovery.clone(),
            ..IndexRequest::default()
        }),
    )
}

async fn stop_workspace(workspace: ResidentWorkspace) -> Result<(), ResidentWorkspaceError> {
    let root = workspace.root.path.clone();
    workspace.cancellation.cancel();
    let close_result =
        workspace
            .session
            .close()
            .await
            .map_err(|source| ResidentWorkspaceError::CloseWatcher {
                root: root.clone(),
                source,
            });
    let task_result = workspace
        .task
        .await
        .map_err(|error| ResidentWorkspaceError::Join {
            root,
            message: error.to_string(),
        })?;
    task_result.and(close_result)
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;
    use zg_engine::{
        ChangeIndexAction, ChangeIndexReply, ChangeIndexRequest, Command, DiscoveryOptions,
        IndexPolicy, IndexRequest, Operation, OperationExecutor, Outcome, Reply, RootSpec,
        RunControl, WorkspaceChange, WorkspaceChangeBatch,
    };
    use zg_testkit::fakes::{ManualWatcher, ScriptedExecutor};

    use super::{ResidentOperationExecutor, ResidentWorkspaceError, ResidentWorkspaceManager};

    #[tokio::test]
    async fn drives_exact_change_batches_and_is_idempotent() {
        let temp = TempDir::new().expect("temp directory");
        let root = root_spec(temp.path().to_path_buf());
        let watcher = Arc::new(ManualWatcher::default());
        let executor = Arc::new(ScriptedExecutor::default());
        executor.respond(
            "index",
            Outcome::Completed(Box::new(Reply::Index(Box::default()))),
        );
        let manager = ResidentWorkspaceManager::new(watcher.clone(), executor.clone());

        manager
            .ensure_watching(root.clone())
            .await
            .expect("first watcher");
        manager
            .ensure_watching(root.clone())
            .await
            .expect("idempotent watcher");
        let changes = vec![
            WorkspaceChange::Delete(PathBuf::from("removed.rs")),
            WorkspaceChange::DeletePrefix(PathBuf::from("generated")),
            WorkspaceChange::RescanDirectory(PathBuf::from("src")),
            WorkspaceChange::Rescan,
        ];
        watcher.push(WorkspaceChangeBatch {
            changes: changes.clone(),
        });

        wait_for_operations(&executor, 1).await;
        assert_eq!(watcher.requests().len(), 1);
        let operations = executor.operations();
        let Command::Index(request) = &operations[0].command else {
            panic!("watch loop must execute an index operation");
        };
        assert_eq!(request.roots.as_slice(), std::slice::from_ref(&root));
        assert_eq!(request.changes, changes);

        assert!(
            manager
                .stop_watching(&root.path)
                .await
                .expect("stop watcher")
        );
        assert!(
            !manager
                .stop_watching(&root.path)
                .await
                .expect("idempotent stop")
        );
    }

    #[tokio::test]
    async fn changed_root_policy_restarts_the_session() {
        let temp = TempDir::new().expect("temp directory");
        let mut root = root_spec(temp.path().to_path_buf());
        let watcher = Arc::new(ManualWatcher::default());
        let executor = Arc::new(ScriptedExecutor::default());
        let manager = ResidentWorkspaceManager::new(watcher.clone(), executor);

        manager
            .ensure_watching(root.clone())
            .await
            .expect("first watcher");
        root.discovery.hidden = true;
        manager
            .ensure_watching(root)
            .await
            .expect("replacement watcher");

        assert_eq!(watcher.requests().len(), 2);
        manager.shutdown_all().await.expect("shutdown");
    }

    #[tokio::test]
    async fn shutdown_closes_sessions_and_rejects_new_roots() {
        let temp = TempDir::new().expect("temp directory");
        let root = root_spec(temp.path().to_path_buf());
        let watcher = Arc::new(ManualWatcher::default());
        let executor = Arc::new(ScriptedExecutor::default());
        let manager = ResidentWorkspaceManager::new(watcher, executor);
        manager
            .ensure_watching(root.clone())
            .await
            .expect("watcher");

        manager.shutdown_all().await.expect("shutdown");
        assert!(matches!(
            manager.ensure_watching(root).await,
            Err(ResidentWorkspaceError::ShuttingDown)
        ));
    }

    #[tokio::test]
    async fn rejects_relative_roots() {
        let manager = ResidentWorkspaceManager::new(
            Arc::new(ManualWatcher::default()),
            Arc::new(ScriptedExecutor::default()),
        );
        let error = manager
            .ensure_watching(root_spec(PathBuf::from("relative")))
            .await
            .expect_err("relative roots must be rejected");
        assert!(matches!(error, ResidentWorkspaceError::RelativeRoot(_)));
    }

    #[tokio::test]
    async fn completed_index_attaches_and_completed_disable_detaches_watcher() {
        let temp = TempDir::new().expect("temp directory");
        let root = root_spec(temp.path().to_path_buf());
        let watcher = Arc::new(ManualWatcher::default());
        let delegate = Arc::new(ScriptedExecutor::default());
        delegate.respond(
            "index",
            Outcome::Completed(Box::new(Reply::Index(Box::default()))),
        );
        delegate.respond(
            "change_index",
            Outcome::Completed(Box::new(Reply::ChangeIndex(Box::new(ChangeIndexReply {
                changed: true,
                index_path: root.path.join(".zvec"),
                policy: IndexPolicy::Disabled,
            })))),
        );
        let residents = ResidentWorkspaceManager::new(watcher.clone(), delegate.clone());
        let executor = ResidentOperationExecutor::new(delegate, residents.clone());

        executor
            .execute(
                Operation::new(
                    root.path.clone(),
                    Command::Index(IndexRequest {
                        roots: vec![root.clone()],
                        ..IndexRequest::default()
                    }),
                ),
                RunControl::local(CancellationToken::new()),
            )
            .await
            .expect("index completion");
        assert_eq!(watcher.requests().len(), 1);

        executor
            .execute(
                Operation::new(
                    root.path.clone(),
                    Command::ChangeIndex(ChangeIndexRequest {
                        action: ChangeIndexAction::Disable,
                        force: false,
                    }),
                ),
                RunControl::local(CancellationToken::new()),
            )
            .await
            .expect("disable completion");
        assert!(
            !residents
                .stop_watching(&root.path)
                .await
                .expect("already detached")
        );
    }

    #[tokio::test]
    async fn accepted_index_does_not_watch_before_job_success() {
        let temp = TempDir::new().expect("temp directory");
        let root = root_spec(temp.path().to_path_buf());
        let watcher = Arc::new(ManualWatcher::default());
        let delegate = Arc::new(ScriptedExecutor::default());
        delegate.respond(
            "index",
            Outcome::Accepted(zg_engine::JobReceipt {
                id: "index-job".to_owned(),
            }),
        );
        let residents = ResidentWorkspaceManager::new(watcher.clone(), delegate.clone());
        let executor = ResidentOperationExecutor::new(delegate, residents);

        executor
            .execute(
                Operation::new(
                    root.path.clone(),
                    Command::Index(IndexRequest {
                        roots: vec![root.clone()],
                        ..IndexRequest::default()
                    }),
                ),
                RunControl::local(CancellationToken::new()),
            )
            .await
            .expect("index accepted");
        assert!(watcher.requests().is_empty());
    }

    fn root_spec(path: PathBuf) -> RootSpec {
        RootSpec {
            path,
            recursive: true,
            discovery: DiscoveryOptions::default(),
        }
    }

    async fn wait_for_operations(executor: &ScriptedExecutor, expected: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while executor.operations().len() < expected {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("resident operation timed out");
    }
}
