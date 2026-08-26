use std::path::Path;

use async_trait::async_trait;

use crate::{CoreError, LexicalSearchReply, LexicalSearchRequest, RunControl};

/// Coarse lexical seam shared by the production ripgrep adapter and test fakes.
#[async_trait]
pub trait LexicalSearchPort: Send + Sync {
    async fn search(
        &self,
        root: &Path,
        request: &LexicalSearchRequest,
        control: &RunControl,
    ) -> Result<LexicalSearchReply, CoreError>;
}
