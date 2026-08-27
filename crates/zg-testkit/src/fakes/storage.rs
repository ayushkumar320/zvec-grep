use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
};

use async_trait::async_trait;
use zg_engine::{
    BeginWriteRequest, Content, CoreError, IndexMutation, IndexSnapshot, IndexStoragePort,
    IndexWritePort, IndexedFile, IndexedFileState, IndexedModelInfo, RecallHit, RecallQuery,
    RecallRequest, RunControl, StoredEntity, WriteMode,
};

#[derive(Clone, Debug, Default)]
pub struct InMemoryStorage {
    inner: Arc<StorageInner>,
}

#[derive(Debug, Default)]
struct StorageInner {
    roots: Mutex<HashMap<PathBuf, RootState>>,
    writers: Mutex<HashSet<PathBuf>>,
}

#[derive(Clone, Debug, Default)]
struct RootState {
    generation: u64,
    model: Option<IndexedModelInfo>,
    files: BTreeMap<PathBuf, IndexedFileState>,
    entities: BTreeMap<String, StoredEntity>,
}

#[async_trait]
impl IndexStoragePort for InMemoryStorage {
    async fn inspect(&self, root: &Path) -> Result<Option<IndexSnapshot>, CoreError> {
        Ok(lock(&self.inner.roots)
            .get(root)
            .map(|state| snapshot(root, state)))
    }

    async fn file_states(
        &self,
        root: &Path,
        paths: &[PathBuf],
        control: &RunControl,
    ) -> Result<Vec<IndexedFileState>, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let roots = lock(&self.inner.roots);
        let Some(state) = roots.get(root) else {
            return Ok(Vec::new());
        };
        if paths.is_empty() {
            return Ok(state.files.values().cloned().collect());
        }
        Ok(paths
            .iter()
            .filter_map(|path| state.files.get(path).cloned())
            .collect())
    }

    async fn recall_batch(
        &self,
        request: &RecallRequest,
        control: &RunControl,
    ) -> Result<Vec<RecallHit>, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let state = lock(&self.inner.roots).get(&request.root).cloned();
        let Some(state) = state else {
            return Ok(Vec::new());
        };
        if request
            .generation
            .is_some_and(|generation| generation != state.generation)
        {
            return Err(CoreError::invalid_input(
                "requested generation is not current",
            ));
        }
        validate_recall_dimensions(&state, request)?;

        let mut hits = Vec::new();
        for route in &request.routes {
            let mut route_hits: Vec<_> = state
                .entities
                .values()
                .filter_map(|entity| {
                    score(entity, &route.query).map(|score| RecallHit {
                        entity_id: entity.entity_id.clone(),
                        file_path: entity.file_path.clone(),
                        range: entity.range.clone(),
                        content: entity.content.clone(),
                        metadata: entity.metadata.clone(),
                        route_id: route.id.clone(),
                        rank: 0,
                        score,
                    })
                })
                .collect();
            route_hits.sort_by(|left, right| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(Ordering::Equal)
                    .then(left.entity_id.cmp(&right.entity_id))
            });
            for (index, hit) in route_hits.iter_mut().enumerate() {
                hit.rank = index + 1;
            }
            route_hits.truncate(request.limit);
            hits.extend(route_hits);
        }
        Ok(hits)
    }

    async fn begin_write(
        &self,
        request: &BeginWriteRequest,
        control: &RunControl,
    ) -> Result<Arc<dyn IndexWritePort>, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        validate_incremental_model(&lock(&self.inner.roots), request)?;
        if !lock(&self.inner.writers).insert(request.root.clone()) {
            return Err(CoreError::backend(
                "in-memory-storage",
                "a writer is already active for this root",
            ));
        }
        Ok(Arc::new(InMemoryWrite {
            inner: Arc::clone(&self.inner),
            request: request.clone(),
            state: Mutex::new(WriteState::default()),
            released: AtomicBool::new(false),
        }))
    }
}

#[derive(Debug, Default)]
struct WriteState {
    mutations: Vec<IndexMutation>,
    finished: bool,
}

#[derive(Debug)]
struct InMemoryWrite {
    inner: Arc<StorageInner>,
    request: BeginWriteRequest,
    state: Mutex<WriteState>,
    released: AtomicBool,
}

#[async_trait]
impl IndexWritePort for InMemoryWrite {
    async fn apply_mutations(
        &self,
        mut mutations: Vec<IndexMutation>,
        control: &RunControl,
    ) -> Result<(), CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        for mutation in &mut mutations {
            validate_mutation(mutation, self.request.model.as_ref())?;
        }
        let mut state = lock(&self.state);
        if state.finished {
            return Err(CoreError::backend(
                "in-memory-storage",
                "write session is already finished",
            ));
        }
        state.mutations.extend(mutations);
        Ok(())
    }

    async fn finalize(&self, control: &RunControl) -> Result<IndexSnapshot, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let mutations = {
            let mut state = lock(&self.state);
            if state.finished {
                return Err(CoreError::backend(
                    "in-memory-storage",
                    "write session is already finished",
                ));
            }
            state.finished = true;
            std::mem::take(&mut state.mutations)
        };

        let snapshot = {
            let mut roots = lock(&self.inner.roots);
            let previous = roots.get(&self.request.root).cloned().unwrap_or_default();
            let mut next = if self.request.mode == WriteMode::Rebuild {
                RootState {
                    generation: previous.generation + 1,
                    model: self.request.model.clone(),
                    ..RootState::default()
                }
            } else {
                RootState {
                    generation: previous.generation + 1,
                    model: self.request.model.clone().or(previous.model),
                    files: previous.files,
                    entities: previous.entities,
                }
            };
            apply_mutations(&mut next, mutations);
            let result = snapshot(&self.request.root, &next);
            roots.insert(self.request.root.clone(), next);
            result
        };
        self.release_writer();
        Ok(snapshot)
    }

    async fn abort(&self) -> Result<(), CoreError> {
        lock(&self.state).finished = true;
        self.release_writer();
        Ok(())
    }
}

impl InMemoryWrite {
    fn release_writer(&self) {
        if !self.released.swap(true, AtomicOrdering::AcqRel) {
            lock(&self.inner.writers).remove(&self.request.root);
        }
    }
}

impl Drop for InMemoryWrite {
    fn drop(&mut self) {
        self.release_writer();
    }
}

fn validate_incremental_model(
    roots: &HashMap<PathBuf, RootState>,
    request: &BeginWriteRequest,
) -> Result<(), CoreError> {
    if request.mode != WriteMode::Incremental {
        return Ok(());
    }
    let Some(existing) = roots
        .get(&request.root)
        .and_then(|state| state.model.as_ref())
    else {
        return Ok(());
    };
    if request
        .model
        .as_ref()
        .is_some_and(|model| model != existing)
    {
        return Err(CoreError::invalid_input(
            "incremental write model does not match the current index",
        ));
    }
    Ok(())
}

fn validate_mutation(
    mutation: &mut IndexMutation,
    model: Option<&IndexedModelInfo>,
) -> Result<(), CoreError> {
    let IndexMutation::ReplaceFile(file) = mutation else {
        return Ok(());
    };
    for entity in &file.entities {
        if entity.file_path != file.relative_path {
            return Err(CoreError::invalid_input(
                "replacement entity path does not match its file state",
            ));
        }
        if let (Some(vector), Some(model)) = (&entity.vector, model)
            && vector.len() != model.dimension
        {
            return Err(CoreError::invalid_input(
                "replacement vector dimension does not match the index model",
            ));
        }
    }
    Ok(())
}

fn apply_mutations(state: &mut RootState, mutations: Vec<IndexMutation>) {
    for mutation in mutations {
        match mutation {
            IndexMutation::ReplaceFile(file) => {
                let IndexedFile {
                    relative_path,
                    source_fingerprint,
                    size_bytes,
                    modified_epoch_ms,
                    entities,
                } = *file;
                let entity_count = entities.len();
                state
                    .entities
                    .retain(|_, entity| entity.file_path != relative_path);
                for entity in entities {
                    state.entities.insert(entity.entity_id.clone(), entity);
                }
                state.files.insert(
                    relative_path.clone(),
                    IndexedFileState {
                        relative_path,
                        source_fingerprint,
                        size_bytes,
                        modified_epoch_ms,
                        entity_count,
                    },
                );
            }
            IndexMutation::DeleteFile(path) => {
                state.files.remove(&path);
                state.entities.retain(|_, entity| entity.file_path != path);
            }
        }
    }
}

fn validate_recall_dimensions(state: &RootState, request: &RecallRequest) -> Result<(), CoreError> {
    let Some(model) = state.model.as_ref() else {
        return Ok(());
    };
    if request.routes.iter().any(|route| {
        matches!(&route.query, RecallQuery::Vector(vector) if vector.len() != model.dimension)
    }) {
        return Err(CoreError::invalid_input(
            "query vector dimension does not match the index model",
        ));
    }
    Ok(())
}

fn score(entity: &StoredEntity, query: &RecallQuery) -> Option<f64> {
    match query {
        RecallQuery::Fts(query) => match &entity.content {
            Content::Text(text) if text.contains(query) => Some(1.0),
            Content::Text(_) | Content::Image(_) => None,
        },
        RecallQuery::Vector(query) => entity.vector.as_ref().map(|vector| {
            vector
                .iter()
                .zip(query)
                .map(|(left, right)| f64::from(*left) * f64::from(*right))
                .sum()
        }),
    }
}

fn snapshot(root: &Path, state: &RootState) -> IndexSnapshot {
    IndexSnapshot {
        root: root.to_path_buf(),
        generation: state.generation,
        index_version: 1,
        model: state.model.clone(),
        file_count: state.files.len(),
        entity_count: state.entities.len(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
