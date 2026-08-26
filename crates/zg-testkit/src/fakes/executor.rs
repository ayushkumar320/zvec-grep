use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use async_trait::async_trait;
use zg_engine::{CoreError, Operation, OperationExecutor, Outcome, RunControl};

#[derive(Debug, Default)]
pub struct ScriptedExecutor {
    replies: Mutex<HashMap<String, Outcome>>,
    operations: Mutex<Vec<Operation>>,
}

impl ScriptedExecutor {
    pub fn respond(&self, capability: impl Into<String>, outcome: Outcome) {
        self.lock_replies().insert(capability.into(), outcome);
    }

    #[must_use]
    pub fn operations(&self) -> Vec<Operation> {
        self.lock_operations().clone()
    }

    fn lock_replies(&self) -> MutexGuard<'_, HashMap<String, Outcome>> {
        match self.replies.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_operations(&self) -> MutexGuard<'_, Vec<Operation>> {
        match self.operations.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[async_trait]
impl OperationExecutor for ScriptedExecutor {
    async fn execute(
        &self,
        operation: Operation,
        control: RunControl,
    ) -> Result<Outcome, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let capability = operation.command.capability().to_owned();
        self.lock_operations().push(operation);
        self.lock_replies()
            .get(&capability)
            .cloned()
            .ok_or(CoreError::CapabilityUnavailable { capability })
    }
}
