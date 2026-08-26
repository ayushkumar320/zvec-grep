use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use tokio::sync::{Notify, Semaphore};
use tracing::debug;

use crate::{
    CURRENT_PROTOCOL_VERSION, Command, CoreConfig, CoreError, CoreEvent, CoreEventKind, Operation,
    Outcome, Reply, RunControl,
};

#[derive(Clone, Debug)]
pub struct Core {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    config: CoreConfig,
    operation_slots: Arc<Semaphore>,
    lifecycle: Mutex<Lifecycle>,
    drained: Notify,
}

#[derive(Debug, Default)]
struct Lifecycle {
    accepting: bool,
    active: usize,
    closed: bool,
}

impl Core {
    /// Opens a Core with adapters supplied by the composition root.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidInput`] when a resource limit is invalid.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub async fn open(config: CoreConfig) -> Result<Self, CoreError> {
        config.resources.validate()?;
        let max_operations = config.resources.max_concurrent_operations;
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                operation_slots: Arc::new(Semaphore::new(max_operations)),
                lifecycle: Mutex::new(Lifecycle {
                    accepting: true,
                    active: 0,
                    closed: false,
                }),
                drained: Notify::new(),
            }),
        })
    }

    /// Executes one typed operation through the shared engine.
    ///
    /// # Errors
    ///
    /// Returns a stable [`CoreError`] for invalid protocol/input, cancellation,
    /// deadlines, lifecycle rejection or adapter failure.
    pub async fn run(
        &self,
        operation: Operation,
        control: RunControl,
    ) -> Result<Outcome, CoreError> {
        if operation.protocol_version != CURRENT_PROTOCOL_VERSION {
            return Err(CoreError::UnsupportedProtocol {
                actual: operation.protocol_version,
                expected: CURRENT_PROTOCOL_VERSION,
            });
        }

        let _active = self.begin_operation()?;
        let operation_id = operation.id;
        let _ = control.events.try_emit(CoreEvent {
            operation_id,
            sequence: 1,
            kind: CoreEventKind::Started,
        });

        let result = match self.acquire_operation_slot(&control).await {
            Ok(permit) => {
                let execution = self.execute(operation, &control);
                tokio::pin!(execution);
                let result = if let Some(deadline) = control.deadline {
                    tokio::select! {
                        () = control.cancellation.cancelled() => Err(CoreError::Cancelled),
                        () = tokio::time::sleep_until(deadline.into()) => Err(CoreError::DeadlineExceeded),
                        outcome = &mut execution => outcome,
                    }
                } else {
                    tokio::select! {
                        () = control.cancellation.cancelled() => Err(CoreError::Cancelled),
                        outcome = &mut execution => outcome,
                    }
                };
                drop(permit);
                result
            }
            Err(error) => Err(error),
        };

        let terminal = match &result {
            Ok(Outcome::Completed(reply)) => CoreEventKind::Completed {
                result_count: reply.result_count(),
            },
            Ok(Outcome::Accepted(_) | Outcome::InputRequired(_)) => {
                CoreEventKind::Completed { result_count: 0 }
            }
            Err(error) => CoreEventKind::Failed { code: error.code() },
        };
        let _ = control.events.try_emit(CoreEvent {
            operation_id,
            sequence: 2,
            kind: terminal,
        });
        result
    }

    /// Stops accepting new work and waits for active operations to drain.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::DeadlineExceeded`] if active operations do not
    /// finish before `deadline`.
    pub async fn shutdown(&self, deadline: Duration) -> Result<(), CoreError> {
        {
            let mut lifecycle = self.lock_lifecycle();
            if lifecycle.closed {
                return Ok(());
            }
            lifecycle.accepting = false;
        }

        let wait_until_drained = async {
            loop {
                let notified = self.inner.drained.notified();
                if self.lock_lifecycle().active == 0 {
                    break;
                }
                notified.await;
            }
        };

        if tokio::time::timeout(deadline, wait_until_drained)
            .await
            .is_err()
        {
            return Err(CoreError::DeadlineExceeded);
        }

        self.lock_lifecycle().closed = true;
        debug!("core shutdown complete");
        Ok(())
    }

    /// Returns the native capabilities registered by this composition root.
    #[must_use]
    pub fn capabilities(&self) -> Vec<&'static str> {
        self.inner.config.ports.capabilities()
    }

    async fn execute(
        &self,
        operation: Operation,
        control: &RunControl,
    ) -> Result<Outcome, CoreError> {
        match operation.command {
            Command::LexicalSearch(request) => self
                .inner
                .config
                .ports
                .lexical()?
                .search(&operation.root, &request, control)
                .await
                .map(|reply| Outcome::Completed(Box::new(Reply::LexicalSearch(Box::new(reply))))),
            command => Err(CoreError::CapabilityUnavailable {
                capability: command.capability().to_owned(),
            }),
        }
    }

    async fn acquire_operation_slot(
        &self,
        control: &RunControl,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, CoreError> {
        let acquire = Arc::clone(&self.inner.operation_slots).acquire_owned();
        tokio::pin!(acquire);

        let permit = if let Some(deadline) = control.deadline {
            tokio::select! {
                () = control.cancellation.cancelled() => return Err(CoreError::Cancelled),
                () = tokio::time::sleep_until(deadline.into()) => return Err(CoreError::DeadlineExceeded),
                permit = &mut acquire => permit,
            }
        } else {
            tokio::select! {
                () = control.cancellation.cancelled() => return Err(CoreError::Cancelled),
                permit = &mut acquire => permit,
            }
        };

        permit.map_err(|_| CoreError::ShuttingDown)
    }

    fn begin_operation(&self) -> Result<ActiveOperation<'_>, CoreError> {
        let mut lifecycle = self.lock_lifecycle();
        if !lifecycle.accepting {
            return Err(CoreError::ShuttingDown);
        }
        lifecycle.active += 1;
        drop(lifecycle);
        Ok(ActiveOperation { core: self })
    }

    fn lock_lifecycle(&self) -> MutexGuard<'_, Lifecycle> {
        match self.inner.lifecycle.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

struct ActiveOperation<'a> {
    core: &'a Core,
}

impl Drop for ActiveOperation<'_> {
    fn drop(&mut self) {
        let mut lifecycle = self.core.lock_lifecycle();
        lifecycle.active = lifecycle.active.saturating_sub(1);
        let drained = lifecycle.active == 0;
        drop(lifecycle);
        if drained {
            self.core.inner.drained.notify_waiters();
        }
    }
}
