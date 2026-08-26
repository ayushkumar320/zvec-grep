use std::path::PathBuf;

use async_trait::async_trait;

use crate::{CoreError, RunControl};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactRequest {
    pub reference: String,
    pub revision: Option<String>,
    pub expected_sha256: Option<String>,
    pub cache_dir: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedArtifact {
    pub path: PathBuf,
    pub resolved_revision: String,
    pub sha256: String,
    pub cache_hit: bool,
}

/// Verified local materialization seam for model and grammar artifacts.
#[async_trait]
pub trait ArtifactSourcePort: Send + Sync {
    async fn materialize_verified(
        &self,
        request: &ArtifactRequest,
        control: &RunControl,
    ) -> Result<MaterializedArtifact, CoreError>;
}
