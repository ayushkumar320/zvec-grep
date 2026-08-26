use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use zg_engine::{
    Content, CoreError, EmbeddingFactoryPort, EmbeddingInput, EmbeddingInputKind, EmbeddingMetric,
    EmbeddingModelInfo, EmbeddingModelSpec, EmbeddingSessionPort, EmbeddingVector, RunControl,
};

#[derive(Debug)]
pub struct DeterministicEmbeddingFactory {
    dimension: usize,
    loaded: Mutex<Vec<EmbeddingModelSpec>>,
}

impl DeterministicEmbeddingFactory {
    #[must_use]
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension: dimension.max(1),
            loaded: Mutex::new(Vec::new()),
        }
    }

    #[must_use]
    pub fn loaded(&self) -> Vec<EmbeddingModelSpec> {
        self.lock_loaded().clone()
    }

    fn lock_loaded(&self) -> MutexGuard<'_, Vec<EmbeddingModelSpec>> {
        match self.loaded.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[async_trait]
impl EmbeddingFactoryPort for DeterministicEmbeddingFactory {
    async fn load(
        &self,
        model: &EmbeddingModelSpec,
        control: &RunControl,
    ) -> Result<Arc<dyn EmbeddingSessionPort>, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        self.lock_loaded().push(model.clone());
        Ok(Arc::new(DeterministicEmbeddingSession::new(
            model,
            self.dimension,
        )))
    }
}

#[derive(Debug)]
pub struct DeterministicEmbeddingSession {
    info: EmbeddingModelInfo,
    closed: AtomicBool,
}

impl DeterministicEmbeddingSession {
    fn new(model: &EmbeddingModelSpec, dimension: usize) -> Self {
        let revision = model.revision.as_deref().unwrap_or("fixture").to_owned();
        Self {
            info: EmbeddingModelInfo {
                reference: model.reference.clone(),
                fingerprint: format!("{}:{revision}:{dimension}", model.reference),
                revision,
                dimension,
                metric: EmbeddingMetric::Cosine,
                input_kinds: vec![EmbeddingInputKind::Text, EmbeddingInputKind::Image],
            },
            closed: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl EmbeddingSessionPort for DeterministicEmbeddingSession {
    fn info(&self) -> &EmbeddingModelInfo {
        &self.info
    }

    async fn embed_batch(
        &self,
        inputs: Vec<EmbeddingInput>,
        control: &RunControl,
    ) -> Result<Vec<EmbeddingVector>, CoreError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(CoreError::backend("fixture-embedding", "session is closed"));
        }
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        Ok(inputs
            .into_iter()
            .map(|input| EmbeddingVector {
                id: input.id,
                values: deterministic_vector(&input.content, self.info.dimension),
                truncated: false,
            })
            .collect())
    }

    async fn close(&self) -> Result<(), CoreError> {
        self.closed.store(true, Ordering::Release);
        Ok(())
    }
}

fn deterministic_vector(content: &Content, dimension: usize) -> Vec<f32> {
    let bytes: &[u8] = match content {
        Content::Text(text) => text.as_bytes(),
        Content::Image(image) => &image.data,
    };
    let mut values = vec![0.0; dimension];
    for (index, byte) in bytes.iter().enumerate() {
        values[index % dimension] += f32::from(*byte) / 255.0;
    }
    values
}
