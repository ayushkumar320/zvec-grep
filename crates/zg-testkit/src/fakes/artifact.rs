use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use async_trait::async_trait;
use zg_engine::{ArtifactRequest, ArtifactSourcePort, CoreError, MaterializedArtifact, RunControl};

#[derive(Debug, Default)]
pub struct FixtureArtifactSource {
    artifacts: Mutex<HashMap<String, MaterializedArtifact>>,
    requests: Mutex<Vec<ArtifactRequest>>,
}

impl FixtureArtifactSource {
    pub fn insert(&self, reference: impl Into<String>, artifact: MaterializedArtifact) {
        self.lock_artifacts().insert(reference.into(), artifact);
    }

    #[must_use]
    pub fn requests(&self) -> Vec<ArtifactRequest> {
        self.lock_requests().clone()
    }

    fn lock_artifacts(&self) -> MutexGuard<'_, HashMap<String, MaterializedArtifact>> {
        match self.artifacts.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_requests(&self) -> MutexGuard<'_, Vec<ArtifactRequest>> {
        match self.requests.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[async_trait]
impl ArtifactSourcePort for FixtureArtifactSource {
    async fn materialize_verified(
        &self,
        request: &ArtifactRequest,
        control: &RunControl,
    ) -> Result<MaterializedArtifact, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        self.lock_requests().push(request.clone());
        self.lock_artifacts()
            .get(&request.reference)
            .cloned()
            .ok_or_else(|| CoreError::CapabilityUnavailable {
                capability: format!("artifact:{}", request.reference),
            })
    }
}
