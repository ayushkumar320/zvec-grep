use async_trait::async_trait;

use crate::{Core, CoreError, Operation, Outcome, RunControl};

/// Transport-facing execution seam implemented by in-process Core, loopback
/// HTTP clients and test fakes.
#[async_trait]
pub trait OperationExecutor: Send + Sync {
    async fn execute(
        &self,
        operation: Operation,
        control: RunControl,
    ) -> Result<Outcome, CoreError>;
}

#[async_trait]
impl OperationExecutor for Core {
    async fn execute(
        &self,
        operation: Operation,
        control: RunControl,
    ) -> Result<Outcome, CoreError> {
        self.run(operation, control).await
    }
}
