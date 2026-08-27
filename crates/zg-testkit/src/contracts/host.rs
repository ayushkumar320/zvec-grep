use std::path::Component;

use tokio_util::sync::CancellationToken;
use zg_engine::{
    CoreError, ReadBatchRequest, RunControl, ScanRequest, WatchRequest, WorkspaceScannerPort,
    WorkspaceWatcherFactoryPort,
};

use super::require;

type ContractResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Verifies deterministic metadata discovery, bounded diagnostics, ordered
/// source reads and cancellation for a scanner adapter.
///
/// # Errors
///
/// Returns a contract violation or adapter error.
pub async fn verify_scanner_contract(
    scanner: &dyn WorkspaceScannerPort,
    request: &ScanRequest,
) -> ContractResult {
    let control = RunControl::local(CancellationToken::new());
    let first = scanner.discover(request, &control).await?;
    let second = scanner.discover(request, &control).await?;
    require(first == second, "repeated discovery must be deterministic")?;
    require(
        first.diagnostics.skipped_samples.len() <= 20,
        "scanner diagnostics must retain at most 20 samples",
    )?;
    require(
        first.diagnostics.skipped_files
            == first.diagnostics.skipped_by_reason.empty
                + first.diagnostics.skipped_by_reason.too_large
                + first.diagnostics.skipped_by_reason.unsupported
                + first.diagnostics.skipped_by_reason.binary,
        "scanner diagnostic totals must agree",
    )?;

    let keys: Vec<_> = first
        .files
        .iter()
        .map(|file| (file.root.clone(), file.relative_path.clone()))
        .collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    require(keys == sorted_keys, "discovered files must be sorted")?;
    require(
        first.files.iter().all(|file| {
            !file.relative_path.as_os_str().is_empty()
                && file
                    .relative_path
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
        }),
        "discovered source paths must be non-empty and relative",
    )?;

    let sources = scanner
        .read_batch(
            &ReadBatchRequest {
                files: first.files.clone(),
            },
            &control,
        )
        .await?;
    require(
        sources.len() == first.files.len(),
        "read_batch must return one source for each discovered file",
    )?;
    for (source, discovered) in sources.iter().zip(&first.files) {
        require(
            source.root == discovered.root
                && source.relative_path == discovered.relative_path
                && source.source_fingerprint == discovered.source_fingerprint,
            "read_batch must preserve discovery order and identity",
        )?;
    }

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let error = scanner
        .discover(request, &RunControl::local(cancelled))
        .await
        .expect_err("a cancelled scan must fail");
    require(
        matches!(error, CoreError::Cancelled),
        "a cancelled scan must return CoreError::Cancelled",
    )?;
    Ok(())
}

/// Verifies cancellation and close semantics shared by watcher adapters.
///
/// # Errors
///
/// Returns a contract violation or adapter error.
pub async fn verify_watcher_lifecycle_contract(
    factory: &dyn WorkspaceWatcherFactoryPort,
    request: &WatchRequest,
) -> ContractResult {
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let Err(error) = factory.watch(request, &RunControl::local(cancelled)).await else {
        return Err("a cancelled watch must fail".into());
    };
    require(
        matches!(error, CoreError::Cancelled),
        "a cancelled watch must return CoreError::Cancelled",
    )?;

    let control = RunControl::local(CancellationToken::new());
    let session = factory.watch(request, &control).await?;
    session.close().await?;
    let error = session
        .next_changes(&control)
        .await
        .expect_err("a closed watch session must reject reads");
    require(
        matches!(error, CoreError::ShuttingDown),
        "a closed watch session must return CoreError::ShuttingDown",
    )?;
    Ok(())
}
