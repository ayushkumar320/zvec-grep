use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;
use zg_engine::{
    Content, ContentRange, IndexMutation, IndexStoragePort, RecallQuery, RecallRequest,
    RecallRoute, RunControl, StoredEntity, WriteMode,
};

use super::require;

/// Verifies single-generation visibility and atomic publication.
///
/// # Errors
///
/// Returns a contract violation or adapter error.
pub async fn verify_storage_contract(
    storage: &dyn IndexStoragePort,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let control = RunControl::local(CancellationToken::new());
    require(
        storage.inspect(root).await?.is_none(),
        "a fresh root must not expose a generation",
    )?;

    let writer = storage
        .begin_write(root, WriteMode::Incremental, &control)
        .await?;
    writer
        .apply_mutations(
            vec![IndexMutation::Upsert(Box::new(StoredEntity {
                entity_id: "entity-1".to_owned(),
                file_path: PathBuf::from("src/a.rs"),
                range: ContentRange::File,
                content: Content::Text("contract needle".to_owned()),
                metadata: None,
                vector: Some(vec![1.0, 0.0]),
            }))],
            &control,
        )
        .await?;
    require(
        storage.inspect(root).await?.is_none(),
        "unfinalized mutations must not be visible",
    )?;
    let snapshot = writer.finalize(&control).await?;
    require(snapshot.generation == 1, "first generation must be one")?;
    require(
        snapshot.entity_count == 1,
        "finalized entity must be visible",
    )?;

    let hits = storage
        .recall_batch(
            &RecallRequest {
                root: root.to_path_buf(),
                generation: Some(snapshot.generation),
                routes: vec![RecallRoute {
                    id: "fts".to_owned(),
                    query: RecallQuery::Fts("needle".to_owned()),
                }],
                limit: 10,
            },
            &control,
        )
        .await?;
    require(hits.len() == 1, "finalized entity must be recalled")?;
    require(hits[0].rank == 1, "recall ranks must start at one")?;

    let rebuild_writer = storage
        .begin_write(root, WriteMode::Rebuild, &control)
        .await?;
    let rebuilt_snapshot = rebuild_writer.finalize(&control).await?;
    require(
        rebuilt_snapshot.generation == 2,
        "rebuild must publish a new generation",
    )?;
    require(
        rebuilt_snapshot.entity_count == 0,
        "rebuild must replace the previous generation",
    )?;
    Ok(())
}
