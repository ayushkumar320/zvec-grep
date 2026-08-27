use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::models::embedding::EmbeddingModelProgress;

#[derive(Clone, Copy, Debug)]
pub(super) struct ArtifactDownloadProgress {
    pub(super) downloaded_bytes: u64,
    pub(super) total_bytes: Option<u64>,
}

#[derive(Clone)]
pub(super) struct ModelDownloadProgressReporter {
    inner: Arc<Inner>,
}

struct Inner {
    model: String,
    on_progress: Option<Arc<dyn Fn(EmbeddingModelProgress) + Send + Sync>>,
    artifacts: Mutex<HashMap<String, ArtifactDownloadProgress>>,
}

impl ModelDownloadProgressReporter {
    pub(super) fn new(
        model: impl Into<String>,
        on_progress: Option<Arc<dyn Fn(EmbeddingModelProgress) + Send + Sync>>,
        expected_artifacts: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                model: model.into(),
                on_progress,
                artifacts: Mutex::new(
                    expected_artifacts
                        .into_iter()
                        .map(|artifact| {
                            (
                                artifact,
                                ArtifactDownloadProgress {
                                    downloaded_bytes: 0,
                                    total_bytes: None,
                                },
                            )
                        })
                        .collect(),
                ),
            }),
        }
    }

    pub(super) fn start(&self) {
        self.emit(EmbeddingModelProgress::Preparing {
            model: self.inner.model.clone(),
        });
    }

    pub(super) fn skip(&self, artifact: &str) {
        self.lock_artifacts().remove(artifact);
    }

    pub(super) fn report(&self, artifact: &str, progress: ArtifactDownloadProgress) {
        let (downloaded_bytes, total_bytes) = {
            let mut artifacts = self.lock_artifacts();
            artifacts.insert(artifact.to_owned(), progress);
            let downloaded_bytes = artifacts.values().map(|value| value.downloaded_bytes).sum();
            let total_bytes = (!artifacts.is_empty()
                && artifacts.values().all(|value| value.total_bytes.is_some()))
            .then(|| {
                artifacts
                    .values()
                    .map(|value| value.total_bytes.unwrap_or_default())
                    .sum()
            });
            (downloaded_bytes, total_bytes)
        };
        self.emit(EmbeddingModelProgress::Downloading {
            model: self.inner.model.clone(),
            downloaded_bytes: Some(downloaded_bytes),
            total_bytes,
        });
    }

    pub(super) fn finish(&self) {
        self.emit(EmbeddingModelProgress::Ready {
            model: self.inner.model.clone(),
        });
    }

    fn emit(&self, event: EmbeddingModelProgress) {
        if let Some(on_progress) = &self.inner.on_progress {
            on_progress(event);
        }
    }

    fn lock_artifacts(&self) -> MutexGuard<'_, HashMap<String, ArtifactDownloadProgress>> {
        match self.inner.artifacts.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::models::embedding::EmbeddingModelProgress;

    use super::{ArtifactDownloadProgress, ModelDownloadProgressReporter};

    #[test]
    fn aggregates_progress_like_typescript() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let reporter = ModelDownloadProgressReporter::new(
            "local/test",
            Some(Arc::new(move |event| {
                captured
                    .lock()
                    .expect("event lock should not be poisoned")
                    .push(event);
            })),
            ["model".to_owned(), "tokenizer".to_owned()],
        );
        reporter.start();
        reporter.report(
            "model",
            ArtifactDownloadProgress {
                downloaded_bytes: 4,
                total_bytes: Some(8),
            },
        );
        reporter.report(
            "tokenizer",
            ArtifactDownloadProgress {
                downloaded_bytes: 4,
                total_bytes: Some(8),
            },
        );
        reporter.finish();

        assert_eq!(
            *events.lock().expect("event lock should not be poisoned"),
            [
                EmbeddingModelProgress::Preparing {
                    model: "local/test".to_owned(),
                },
                EmbeddingModelProgress::Downloading {
                    model: "local/test".to_owned(),
                    downloaded_bytes: Some(4),
                    total_bytes: None,
                },
                EmbeddingModelProgress::Downloading {
                    model: "local/test".to_owned(),
                    downloaded_bytes: Some(8),
                    total_bytes: Some(16),
                },
                EmbeddingModelProgress::Ready {
                    model: "local/test".to_owned(),
                },
            ]
        );
    }
}
