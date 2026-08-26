use std::path::Path;

use tokio_util::sync::CancellationToken;
use zg_engine::{LexicalSearchPort, LexicalSearchRequest, RunControl};

use super::require;

/// Runs the behavior shared by production and fake lexical adapters.
///
/// # Errors
///
/// Returns a contract violation or adapter error when observable behavior does
/// not satisfy the lexical seam.
pub async fn verify_lexical_contract(
    port: &dyn LexicalSearchPort,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = LexicalSearchRequest {
        patterns: vec!["contract-needle".to_owned()],
        limit: Some(5),
        ..LexicalSearchRequest::default()
    };
    let reply = port
        .search(root, &request, &RunControl::local(CancellationToken::new()))
        .await?;
    require(reply.root == root, "reply root differs from request root")?;
    require(
        reply.diagnostics.limit == request.limit,
        "diagnostics must preserve the requested limit",
    )?;
    for (index, item) in reply.matches.iter().enumerate() {
        require(
            item.rank == index + 1,
            format!("rank {} is not deterministic", item.rank),
        )?;
    }
    Ok(())
}
