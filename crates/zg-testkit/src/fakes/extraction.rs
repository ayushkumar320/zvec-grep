use std::sync::{Mutex, MutexGuard};

use async_trait::async_trait;
use zg_engine::{
    Content, ContentRange, CoreError, ExtractInput, ExtractedDocument, ExtractedEntity,
    ExtractionPort, ExtractionWarning, FileKind, RunControl,
};

#[derive(Debug, Default)]
pub struct FixtureExtraction {
    batches: Mutex<Vec<Vec<ExtractInput>>>,
}

impl FixtureExtraction {
    #[must_use]
    pub fn batches(&self) -> Vec<Vec<ExtractInput>> {
        self.lock_batches().clone()
    }

    fn lock_batches(&self) -> MutexGuard<'_, Vec<Vec<ExtractInput>>> {
        match self.batches.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[async_trait]
impl ExtractionPort for FixtureExtraction {
    async fn extract_batch(
        &self,
        inputs: Vec<ExtractInput>,
        control: &RunControl,
    ) -> Result<Vec<ExtractedDocument>, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        self.lock_batches().push(inputs.clone());
        Ok(inputs.into_iter().map(extract_fixture).collect())
    }
}

fn extract_fixture(input: ExtractInput) -> ExtractedDocument {
    match String::from_utf8(input.bytes) {
        Ok(text) => ExtractedDocument {
            path: input.path.clone(),
            kind: input.kind_hint.unwrap_or(FileKind::Text),
            format: input.format_hint.unwrap_or_else(|| "text".to_owned()),
            entities: vec![ExtractedEntity {
                stable_id: format!("fixture:{}", input.path.display()),
                range: ContentRange::File,
                content: Content::Text(text),
                metadata: None,
            }],
            warnings: Vec::new(),
        },
        Err(_) => ExtractedDocument {
            path: input.path,
            kind: input.kind_hint.unwrap_or(FileKind::Data),
            format: input.format_hint.unwrap_or_else(|| "binary".to_owned()),
            entities: Vec::new(),
            warnings: vec![ExtractionWarning {
                code: "invalid_utf8".to_owned(),
                message: "fixture input is not valid UTF-8".to_owned(),
            }],
        },
    }
}
