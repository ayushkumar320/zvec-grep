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
    Content, CoreError, IndexMutation, IndexSnapshot, IndexStoragePort, IndexWritePort, RecallHit,
    RecallQuery, RecallRequest, RunControl, StoredEntity, WriteMode,
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
    entities: BTreeMap<String, StoredEntity>,
}

#[async_trait]
impl IndexStoragePort for InMemoryStorage {
    async fn inspect(&self, root: &Path) -> Result<Option<IndexSnapshot>, CoreError> {
        Ok(lock(&self.inner.roots)
            .get(root)
            .map(|state| snapshot(root, state)))
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
        root: &Path,
        mode: WriteMode,
        control: &RunControl,
    ) -> Result<Arc<dyn IndexWritePort>, CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        let root = root.to_path_buf();
        if !lock(&self.inner.writers).insert(root.clone()) {
            return Err(CoreError::backend(
                "in-memory-storage",
                "a writer is already active for this root",
            ));
        }
        Ok(Arc::new(InMemoryWrite {
            inner: Arc::clone(&self.inner),
            root,
            mode,
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
    root: PathBuf,
    mode: WriteMode,
    state: Mutex<WriteState>,
    released: AtomicBool,
}

#[async_trait]
impl IndexWritePort for InMemoryWrite {
    async fn apply_mutations(
        &self,
        mutations: Vec<IndexMutation>,
        control: &RunControl,
    ) -> Result<(), CoreError> {
        if control.cancellation.is_cancelled() {
            return Err(CoreError::Cancelled);
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
            let previous = roots.get(&self.root).cloned().unwrap_or_default();
            let mut next = RootState {
                generation: previous.generation + 1,
                entities: if self.mode == WriteMode::Rebuild {
                    BTreeMap::new()
                } else {
                    previous.entities
                },
            };
            apply_mutations(&mut next.entities, mutations);
            let result = snapshot(&self.root, &next);
            roots.insert(self.root.clone(), next);
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
            lock(&self.inner.writers).remove(&self.root);
        }
    }
}

impl Drop for InMemoryWrite {
    fn drop(&mut self) {
        self.release_writer();
    }
}

fn apply_mutations(entities: &mut BTreeMap<String, StoredEntity>, mutations: Vec<IndexMutation>) {
    for mutation in mutations {
        match mutation {
            IndexMutation::Upsert(entity) => {
                entities.insert(entity.entity_id.clone(), *entity);
            }
            IndexMutation::DeleteFile(path) => {
                entities.retain(|_, entity| entity.file_path != path);
            }
        }
    }
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
        model_fingerprint: None,
        entity_count: state.entities.len(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
