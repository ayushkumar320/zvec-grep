use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;
use zg_engine::{ExtractInput, ExtractionPort, FileKind, RunControl};

use super::require;

/// Verifies deterministic, input-ordered batch extraction.
///
/// # Errors
///
/// Returns a contract violation or adapter error.
pub async fn verify_extraction_contract(
    port: &dyn ExtractionPort,
) -> Result<(), Box<dyn std::error::Error>> {
    let inputs = vec![
        ExtractInput {
            path: PathBuf::from("a.txt"),
            bytes: b"alpha".to_vec(),
            kind_hint: Some(FileKind::Text),
            format_hint: Some("text".to_owned()),
        },
        ExtractInput {
            path: PathBuf::from("b.md"),
            bytes: b"# beta".to_vec(),
            kind_hint: Some(FileKind::Text),
            format_hint: Some("markdown".to_owned()),
        },
    ];
    let control = RunControl::local(CancellationToken::new());
    let first = port.extract_batch(inputs.clone(), &control).await?;
    let second = port.extract_batch(inputs, &control).await?;
    require(
        first == second,
        "same inputs must produce the same extraction",
    )?;
    require(first.len() == 2, "one result is required for every input")?;
    require(
        first[0].path.as_path() == Path::new("a.txt")
            && first[1].path.as_path() == Path::new("b.md"),
        "batch result order must match input order",
    )?;
    Ok(())
}
