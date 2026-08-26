mod artifact;
mod embedding;
mod extraction;
mod lexical;
mod storage;

use thiserror::Error;

pub use artifact::verify_artifact_source_contract;
pub use embedding::verify_embedding_contract;
pub use extraction::verify_extraction_contract;
pub use lexical::verify_lexical_contract;
pub use storage::verify_storage_contract;

#[derive(Debug, Error)]
#[error("adapter contract violation: {0}")]
pub struct ContractError(pub String);

fn require(condition: bool, message: impl Into<String>) -> Result<(), ContractError> {
    if condition {
        Ok(())
    } else {
        Err(ContractError(message.into()))
    }
}
