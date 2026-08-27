use std::{
    error::Error,
    path::{Path, PathBuf},
};

use tokio_util::sync::CancellationToken;
use zg_engine::{
    BeginWriteRequest, Content, ContentRange, EmbeddingMetric, IndexMutation, IndexSnapshot,
    IndexStoragePort, IndexedFile, IndexedModelInfo, RecallQuery, RecallRequest, RecallRoute,
    RunControl, StoredEntity, WriteMode,
};

use super::require;

type ContractResult<T = ()> = Result<T, Box<dyn Error>>;

/// Verifies file replacement, model metadata and atomic publication.
///
/// # Errors
///
/// Returns a contract violation or adapter error.
pub async fn verify_storage_contract(
    storage: &dyn IndexStoragePort,
    root: &Path,
) -> ContractResult {
    let control = RunControl::local(CancellationToken::new());
    require(
        storage.inspect(root).await?.is_none(),
        "a fresh root must not expose a generation",
    )?;

    let model = fixture_model();
    let initial = publish_initial(storage, root, &model, &control).await?;
    verify_recall(storage, root, initial.generation, "needle", 1, &control).await?;

    let replaced = replace_file_generation(storage, root, &model, &control).await?;
    require(
        replaced.entity_count == 1,
        "file replacement must remove stale entities",
    )?;
    verify_recall(storage, root, replaced.generation, "needle", 0, &control).await?;

    verify_rebuild(storage, root, model, &control).await
}

async fn publish_initial(
    storage: &dyn IndexStoragePort,
    root: &Path,
    model: &IndexedModelInfo,
    control: &RunControl,
) -> ContractResult<IndexSnapshot> {
    let writer = storage
        .begin_write(
            &write_request(root, WriteMode::Incremental, model.clone()),
            control,
        )
        .await?;
    writer
        .apply_mutations(
            vec![replace_file(
                "src/a.rs",
                "fingerprint-1",
                vec![entity("entity-1", "src/a.rs", "contract needle")],
            )],
            control,
        )
        .await?;
    require(
        storage.inspect(root).await?.is_none(),
        "unfinalized mutations must not be visible",
    )?;
    require(
        storage.file_states(root, &[], control).await?.is_empty(),
        "unfinalized file states must not be visible",
    )?;

    let snapshot = writer.finalize(control).await?;
    require(snapshot.generation == 1, "first generation must be one")?;
    require(snapshot.file_count == 1, "finalized file must be visible")?;
    require(
        snapshot.entity_count == 1,
        "finalized entity must be visible",
    )?;
    require(
        snapshot.model.as_ref() == Some(model),
        "the generation must preserve model metadata",
    )?;
    let files = storage.file_states(root, &[], control).await?;
    require(files.len() == 1, "one file state must be published")?;
    require(
        files[0].source_fingerprint == "fingerprint-1",
        "source fingerprint must be preserved",
    )?;
    Ok(snapshot)
}

async fn replace_file_generation(
    storage: &dyn IndexStoragePort,
    root: &Path,
    model: &IndexedModelInfo,
    control: &RunControl,
) -> ContractResult<IndexSnapshot> {
    let writer = storage
        .begin_write(
            &write_request(root, WriteMode::Incremental, model.clone()),
            control,
        )
        .await?;
    writer
        .apply_mutations(
            vec![replace_file(
                "src/a.rs",
                "fingerprint-2",
                vec![entity("entity-2", "src/a.rs", "replacement")],
            )],
            control,
        )
        .await?;
    Ok(writer.finalize(control).await?)
}

async fn verify_recall(
    storage: &dyn IndexStoragePort,
    root: &Path,
    generation: u64,
    query: &str,
    expected: usize,
    control: &RunControl,
) -> ContractResult {
    let hits = storage
        .recall_batch(
            &RecallRequest {
                root: root.to_path_buf(),
                generation: Some(generation),
                routes: vec![RecallRoute {
                    id: "fts".to_owned(),
                    query: RecallQuery::Fts(query.to_owned()),
                }],
                limit: 10,
            },
            control,
        )
        .await?;
    require(hits.len() == expected, "recall result count differs")?;
    if let Some(hit) = hits.first() {
        require(hit.rank == 1, "recall ranks must start at one")?;
    }
    Ok(())
}

async fn verify_rebuild(
    storage: &dyn IndexStoragePort,
    root: &Path,
    model: IndexedModelInfo,
    control: &RunControl,
) -> ContractResult {
    let writer = storage
        .begin_write(&write_request(root, WriteMode::Rebuild, model), control)
        .await?;
    let snapshot = writer.finalize(control).await?;
    require(
        snapshot.generation == 3,
        "rebuild must publish a new generation",
    )?;
    require(
        snapshot.file_count == 0 && snapshot.entity_count == 0,
        "rebuild must replace the previous generation",
    )?;
    Ok(())
}

fn fixture_model() -> IndexedModelInfo {
    IndexedModelInfo {
        fingerprint: "fixture:model:v1".to_owned(),
        dimension: 2,
        metric: EmbeddingMetric::Cosine,
    }
}

fn write_request(root: &Path, mode: WriteMode, model: IndexedModelInfo) -> BeginWriteRequest {
    BeginWriteRequest {
        root: root.to_path_buf(),
        mode,
        model: Some(model),
    }
}

fn replace_file(path: &str, fingerprint: &str, entities: Vec<StoredEntity>) -> IndexMutation {
    IndexMutation::ReplaceFile(Box::new(IndexedFile {
        relative_path: PathBuf::from(path),
        source_fingerprint: fingerprint.to_owned(),
        size_bytes: 10,
        modified_epoch_ms: Some(1),
        entities,
    }))
}

fn entity(id: &str, path: &str, content: &str) -> StoredEntity {
    StoredEntity {
        entity_id: id.to_owned(),
        file_path: PathBuf::from(path),
        range: ContentRange::File,
        content: Content::Text(content.to_owned()),
        metadata: None,
        vector: Some(vec![1.0, 0.0]),
    }
}
