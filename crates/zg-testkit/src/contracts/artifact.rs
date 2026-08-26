use tokio_util::sync::CancellationToken;
use zg_engine::{ArtifactRequest, ArtifactSourcePort, MaterializedArtifact, RunControl};

use super::require;

/// Verifies deterministic verified materialization and cache identity.
///
/// # Errors
///
/// Returns a contract violation or adapter error.
pub async fn verify_artifact_source_contract(
    source: &dyn ArtifactSourcePort,
    request: &ArtifactRequest,
    expected: &MaterializedArtifact,
) -> Result<(), Box<dyn std::error::Error>> {
    let control = RunControl::local(CancellationToken::new());
    let first = source.materialize_verified(request, &control).await?;
    let second = source.materialize_verified(request, &control).await?;
    require(
        &first == expected,
        "materialized artifact differs from expected",
    )?;
    require(
        first.path == second.path
            && first.sha256 == second.sha256
            && first.resolved_revision == second.resolved_revision,
        "repeated materialization must preserve artifact identity",
    )?;
    Ok(())
}
