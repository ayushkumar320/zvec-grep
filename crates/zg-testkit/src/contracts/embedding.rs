use tokio_util::sync::CancellationToken;
use zg_engine::{
    Content, Device, EmbeddingFactoryPort, EmbeddingInput, EmbeddingModelSpec, RunControl,
};

use super::require;

/// Verifies model metadata, determinism and vector dimensions.
///
/// # Errors
///
/// Returns a contract violation or adapter error.
pub async fn verify_embedding_contract(
    factory: &dyn EmbeddingFactoryPort,
) -> Result<(), Box<dyn std::error::Error>> {
    let control = RunControl::local(CancellationToken::new());
    let session = factory
        .load(
            &EmbeddingModelSpec {
                reference: "fixture/model".to_owned(),
                revision: Some("contract".to_owned()),
                cache_dir: None,
                endpoint: None,
                device: Device::Cpu,
            },
            &control,
        )
        .await?;
    require(
        session.info().dimension > 0,
        "model dimension must be positive",
    )?;
    require(
        !session.info().fingerprint.is_empty(),
        "model fingerprint must not be empty",
    )?;
    let inputs = vec![EmbeddingInput {
        id: "one".to_owned(),
        content: Content::Text("contract input".to_owned()),
    }];
    let first = session.embed_batch(inputs.clone(), &control).await?;
    let second = session.embed_batch(inputs, &control).await?;
    require(first == second, "same inputs must produce the same vectors")?;
    require(first.len() == 1, "one vector is required for every input")?;
    require(
        first[0].values.len() == session.info().dimension,
        "vector dimension differs from model metadata",
    )?;
    require(
        first[0].values.iter().all(|value| value.is_finite()),
        "vectors must contain only finite values",
    )?;
    session.close().await?;
    Ok(())
}
