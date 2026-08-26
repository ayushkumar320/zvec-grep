use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use async_trait::async_trait;
use zg_engine::{
    CoreError, LexicalCoverage, LexicalDiagnostics, LexicalSearchPort, LexicalSearchReply,
    LexicalSearchRequest, RunControl,
};

#[derive(Debug, Default)]
pub struct RecordedLexical {
    requests: Mutex<Vec<(PathBuf, LexicalSearchRequest)>>,
}

impl RecordedLexical {
    #[must_use]
    pub fn requests(&self) -> Vec<(PathBuf, LexicalSearchRequest)> {
        self.lock_requests().clone()
    }

    fn lock_requests(&self) -> MutexGuard<'_, Vec<(PathBuf, LexicalSearchRequest)>> {
        match self.requests.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[async_trait]
impl LexicalSearchPort for RecordedLexical {
    async fn search(
        &self,
        root: &Path,
        request: &LexicalSearchRequest,
        _control: &RunControl,
    ) -> Result<LexicalSearchReply, CoreError> {
        self.lock_requests()
            .push((root.to_path_buf(), request.clone()));
        Ok(LexicalSearchReply {
            root: root.to_path_buf(),
            coverage: LexicalCoverage::Exhaustive,
            matches: Vec::new(),
            diagnostics: LexicalDiagnostics {
                backend: "recorded-fake".to_owned(),
                command: PathBuf::from("fake-rg"),
                args: Vec::new(),
                ignored_directories: Vec::new(),
                missing_paths: Vec::new(),
                searched_paths: request.paths.clone(),
                limit: request.limit,
                truncated: false,
            },
        })
    }
}
