use std::{
    env,
    fmt::{self, Write as _},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use uuid::Uuid;
use zg_host_native::NativeScanner;

use crate::{
    EngineError,
    api::{
        index::{
            IndexOptions, IndexResult,
            options::{Device, DiscoveryOptions, EmbeddingModelSpec, RootPath},
        },
        info::{
            InfoOptions, InfoResult,
            result::{
                InfoSource, WorkspaceIndexEmbedding, WorkspaceIndexInfo, WorkspaceIndexPolicy,
            },
        },
    },
    models::{
        CreateEmbeddingModelOptions, EmbeddingMetric, ModelError, ModelRuntimeLease,
        ModelRuntimeManager, ModelRuntimeRequest,
    },
    storage::spi::{
        WorkspaceIndexEmbeddingSchema, WorkspaceIndexStorageFactory, WorkspaceIndexStorageOptions,
    },
    workspace::{
        layout::{
            WorkspaceIndexLocation, find_nearest_workspace, reset_workspace_index,
            workspace_index_location,
        },
        lock::{LockMode, acquire_home_lock},
        manifest::{
            EmbeddingRuntimeConfig, WorkspaceManifest, read_workspace_manifest,
            write_workspace_manifest,
        },
    },
};

use super::pipeline::{IndexingContext, get_workspace_index_status, index_workspace};

const DEFAULT_LOCAL_EMBEDDING: &str = "local/potion-code-16m-v2";
const CURRENT_INDEX_VERSION: u32 = 1;

#[derive(Clone)]
pub(crate) struct WorkspaceIndexService {
    scanner: NativeScanner,
    storage_factory: Option<Arc<dyn WorkspaceIndexStorageFactory>>,
}

impl WorkspaceIndexService {
    pub(crate) fn new() -> Self {
        Self {
            scanner: NativeScanner::default(),
            storage_factory: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_storage_factory(
        storage_factory: Arc<dyn WorkspaceIndexStorageFactory>,
    ) -> Self {
        Self {
            scanner: NativeScanner::default(),
            storage_factory: Some(storage_factory),
        }
    }

    pub(crate) async fn index(
        &self,
        models: &ModelRuntimeManager,
        options: IndexOptions,
    ) -> Result<IndexResult, EngineError> {
        let factory = self.storage_factory()?;
        let location = workspace_index_location_from_option(options.root.as_deref())?;
        let _lock = acquire_home_lock(
            &location.home,
            LockMode::Write,
            if options.rebuild {
                "index.rebuild"
            } else {
                "index"
            },
        )?;
        let existing = read_workspace_manifest(&location.home)?;
        let model = acquire_model(models, existing.as_ref(), &options)?;
        if !options.rebuild {
            assert_embedding_compatible(existing.as_ref(), &model)?;
        }

        if options.rebuild
            || existing
                .as_ref()
                .is_none_or(|manifest| !is_indexed(manifest))
        {
            reset_workspace_index(&location, factory.as_ref())?;
        }
        let existing_for_manifest = if options.rebuild {
            None
        } else {
            existing.as_ref()
        };
        let roots = resolve_root_paths(&location.root, existing_for_manifest, &options);
        let now = epoch_millis();
        let info = WorkspaceIndexInfo {
            id: existing_for_manifest.map_or_else(
                || Uuid::new_v4().to_string(),
                |manifest| manifest.id.clone(),
            ),
            name: existing_for_manifest.map_or_else(
                || workspace_name(&location.root),
                |manifest| manifest.name.clone(),
            ),
            path: location.home.clone(),
            roots,
            policy: WorkspaceIndexPolicy::Enabled,
            embedding: Some(embedding_schema(&model)),
            index_version: Some(CURRENT_INDEX_VERSION),
            generation: existing_for_manifest.and_then(|manifest| manifest.index_info().generation),
            created_epoch_ms: existing_for_manifest.map_or(now, |manifest| manifest.created_time),
            updated_epoch_ms: now,
        };
        let runtime = embedding_runtime(existing_for_manifest, &options, &model);
        let mut manifest = WorkspaceManifest::new(info.clone(), runtime)?;
        let storage = factory.open(WorkspaceIndexStorageOptions::ReadWrite {
            storage_path: location.home.clone(),
            embedding: storage_embedding_schema(&model),
        })?;
        if let Err(error) = write_workspace_manifest(&location.home, &manifest) {
            let _ = storage.close();
            return Err(error);
        }

        let result = index_workspace(&IndexingContext {
            workspace_index: &info,
            storage: storage.as_ref(),
            scanner: &self.scanner,
            embedding_model: &model,
            embedding_concurrency: options.embedding_concurrency,
            on_progress: options.on_progress,
            signal: None,
            changes: &options.changes,
        })
        .await;
        manifest.updated_time = epoch_millis();
        let manifest_result = write_workspace_manifest(&location.home, &manifest);
        let close_result = storage.close();

        let indexed = result?;
        manifest_result?;
        close_result?;
        Ok(indexed)
    }

    pub(crate) async fn info(&self, options: InfoOptions) -> Result<InfoResult, EngineError> {
        let requested_root = resolve_root(options.root.as_deref())?;
        let requested_location = workspace_index_location(&requested_root)?;
        let Some(location) = find_nearest_workspace(&requested_root)? else {
            return Ok(unindexed_info(
                requested_location,
                WorkspaceIndexPolicy::Undecided,
            ));
        };
        let _lock = acquire_home_lock(&location.home, LockMode::Read, "info")?;
        let Some(manifest) = read_workspace_manifest(&location.home)? else {
            return Ok(unindexed_info(location, WorkspaceIndexPolicy::Undecided));
        };
        let metadata_indexed = is_indexed(&manifest);
        let storage_exists = match &self.storage_factory {
            Some(factory) => factory.exists(&location.home)?,
            None => false,
        };
        let indexed = metadata_indexed && storage_exists;
        let status = if options.include_status && indexed {
            let factory = self.storage_factory()?;
            let storage = factory.open(WorkspaceIndexStorageOptions::ReadOnly {
                storage_path: location.home.clone(),
            })?;
            let status = get_workspace_index_status(
                &manifest.index_info(),
                storage.as_ref(),
                &self.scanner,
                None,
            )
            .await;
            let close = storage.close();
            let status = status?;
            close?;
            Some(status)
        } else {
            None
        };

        Ok(InfoResult {
            root: location.root,
            indexed,
            index_policy: manifest.index_policy,
            home: location.home,
            index_path: location.index_path,
            source: if indexed {
                InfoSource::Index
            } else {
                InfoSource::Unindexed
            },
            workspace_index: Some(manifest.index_info()),
            status,
            suggestion: workspace_suggestion(&manifest, indexed),
        })
    }

    pub(crate) fn drop_index(&self, options: &InfoOptions) -> Result<bool, EngineError> {
        let location = workspace_index_location_from_option(options.root.as_deref())?;
        if read_workspace_manifest(&location.home)?.is_none() {
            return Ok(false);
        }
        let _lock = acquire_home_lock(&location.home, LockMode::Write, "index.drop")?;
        if read_workspace_manifest(&location.home)?.is_none() {
            return Ok(false);
        }
        let factory = self.storage_factory()?;
        reset_workspace_index(&location, factory.as_ref())?;
        Ok(true)
    }

    pub(crate) fn disable_index(options: &InfoOptions) -> Result<InfoResult, EngineError> {
        let location = workspace_index_location_from_option(options.root.as_deref())?;
        let _lock = acquire_home_lock(&location.home, LockMode::Write, "index.disable")?;
        let existing = read_workspace_manifest(&location.home)?;
        let now = epoch_millis();
        let manifest = WorkspaceManifest::new(
            WorkspaceIndexInfo {
                id: existing.as_ref().map_or_else(
                    || Uuid::new_v4().to_string(),
                    |manifest| manifest.id.clone(),
                ),
                name: existing.as_ref().map_or_else(
                    || workspace_name(&location.root),
                    |manifest| manifest.name.clone(),
                ),
                path: location.home.clone(),
                roots: existing.as_ref().map_or_else(
                    || vec![default_root_path(&location.root)],
                    |manifest| manifest.index_info().roots,
                ),
                policy: WorkspaceIndexPolicy::Disabled,
                embedding: None,
                index_version: None,
                generation: None,
                created_epoch_ms: existing
                    .as_ref()
                    .map_or(now, |manifest| manifest.created_time),
                updated_epoch_ms: now,
            },
            existing.map_or_else(EmbeddingRuntimeConfig::default, |manifest| {
                manifest.embedding_runtime
            }),
        )?;
        write_workspace_manifest(&location.home, &manifest)?;
        Ok(InfoResult {
            root: location.root,
            indexed: false,
            index_policy: WorkspaceIndexPolicy::Disabled,
            home: location.home,
            index_path: location.index_path,
            source: InfoSource::Unindexed,
            workspace_index: Some(manifest.index_info()),
            status: None,
            suggestion: Some("indexing is disabled for this workspace".to_owned()),
        })
    }

    fn storage_factory(&self) -> Result<&Arc<dyn WorkspaceIndexStorageFactory>, EngineError> {
        self.storage_factory
            .as_ref()
            .ok_or_else(|| EngineError::CapabilityUnavailable {
                capability: "workspace storage backend".to_owned(),
            })
    }
}

impl Default for WorkspaceIndexService {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for WorkspaceIndexService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceIndexService")
            .field("scanner", &self.scanner)
            .field("storage_configured", &self.storage_factory.is_some())
            .finish()
    }
}

fn acquire_model(
    models: &ModelRuntimeManager,
    existing: Option<&WorkspaceManifest>,
    options: &IndexOptions,
) -> Result<ModelRuntimeLease, EngineError> {
    if options
        .embedding
        .as_ref()
        .and_then(|embedding| embedding.revision.as_ref())
        .is_some()
    {
        return Err(EngineError::invalid_input(
            "embedding revision overrides are not supported by the catalog-backed runtime",
        ));
    }
    let reference = embedding_reference(existing, options.embedding.as_ref());
    let local = reference.starts_with("local/");
    let existing_runtime = existing.map(|manifest| &manifest.embedding_runtime);
    let api_key = if local {
        None
    } else {
        existing_runtime
            .and_then(|runtime| runtime.api_key.clone())
            .or_else(environment_api_key)
    };
    let endpoint = options
        .embedding
        .as_ref()
        .and_then(|embedding| embedding.endpoint.clone())
        .or_else(|| existing_runtime.and_then(|runtime| runtime.endpoint.clone()));
    let device = local.then(|| {
        options.embedding.as_ref().map_or_else(
            || {
                existing_runtime
                    .and_then(|runtime| runtime.device)
                    .unwrap_or(Device::Auto)
            },
            |embedding| embedding.device,
        )
    });
    models
        .acquire(ModelRuntimeRequest::new(
            reference,
            CreateEmbeddingModelOptions {
                api_key,
                endpoint,
                model_cache_dir: options
                    .embedding
                    .as_ref()
                    .and_then(|embedding| embedding.cache_dir.clone()),
                device,
                ..CreateEmbeddingModelOptions::default()
            },
            options.embedding_concurrency,
        ))
        .map_err(|error| model_error(&error))
}

fn embedding_reference(
    existing: Option<&WorkspaceManifest>,
    requested: Option<&EmbeddingModelSpec>,
) -> String {
    requested
        .map(|embedding| embedding.reference.clone())
        .or_else(|| {
            existing.and_then(|manifest| {
                manifest
                    .embedding
                    .as_ref()
                    .map(|embedding| format!("{}/{}", embedding.provider, embedding.model))
            })
        })
        .or_else(|| {
            env::var("ZVEC_GREP_EMBEDDING")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_LOCAL_EMBEDDING.to_owned())
}

fn environment_api_key() -> Option<String> {
    ["ZVEC_GREP_API_KEY", "DASHSCOPE_API_KEY", "QWEN_API_KEY"]
        .into_iter()
        .find_map(|name| {
            env::var(name)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
}

fn assert_embedding_compatible(
    existing: Option<&WorkspaceManifest>,
    model: &ModelRuntimeLease,
) -> Result<(), EngineError> {
    let Some(existing) = existing.filter(|manifest| is_indexed(manifest)) else {
        return Ok(());
    };
    let Some(schema) = &existing.embedding else {
        return Ok(());
    };
    let current = model.info();
    if schema.provider == current.provider
        && schema.model == current.name
        && schema.dimension == current.dimension
        && schema.metric == metric_name(current.metric)
    {
        return Ok(());
    }
    Err(EngineError::invalid_input(
        "existing index uses a different embedding model; rebuild the index",
    ))
}

fn resolve_root_paths(
    workspace_root: &Path,
    existing: Option<&WorkspaceManifest>,
    options: &IndexOptions,
) -> Vec<RootPath> {
    let mut roots = if options.roots.is_empty() {
        existing.map_or_else(
            || vec![default_root_path(workspace_root)],
            |manifest| manifest.index_info().roots,
        )
    } else {
        options.roots.clone()
    };
    for root in &mut roots {
        if !root.path.is_absolute() {
            root.path = workspace_root.join(&root.path);
        }
        if options.reset_paths {
            root.discovery = DiscoveryOptions::default();
        }
        apply_discovery_overrides(&mut root.discovery, &options.discovery);
    }
    roots
}

fn apply_discovery_overrides(target: &mut DiscoveryOptions, overrides: &DiscoveryOptions) {
    if !overrides.include_paths.is_empty() {
        target.include_paths.clone_from(&overrides.include_paths);
    }
    if !overrides.exclude_paths.is_empty() {
        target.exclude_paths.clone_from(&overrides.exclude_paths);
    }
    if !overrides.globs.is_empty() {
        target.globs.clone_from(&overrides.globs);
    }
    if !overrides.insensitive_globs.is_empty() {
        target
            .insensitive_globs
            .clone_from(&overrides.insensitive_globs);
    }
    if !overrides.file_types.is_empty() {
        target.file_types.clone_from(&overrides.file_types);
    }
    if !overrides.excluded_file_types.is_empty() {
        target
            .excluded_file_types
            .clone_from(&overrides.excluded_file_types);
    }
    if !overrides.ignore_files.is_empty() {
        target.ignore_files.clone_from(&overrides.ignore_files);
    }
    target.hidden |= overrides.hidden;
    target.no_ignore |= overrides.no_ignore;
    target.follow |= overrides.follow;
    if overrides.max_depth.is_some() {
        target.max_depth = overrides.max_depth;
    }
    if overrides.max_file_size_bytes.is_some() {
        target.max_file_size_bytes = overrides.max_file_size_bytes;
    }
}

fn embedding_runtime(
    existing: Option<&WorkspaceManifest>,
    options: &IndexOptions,
    model: &ModelRuntimeLease,
) -> EmbeddingRuntimeConfig {
    let current = existing
        .map(|manifest| manifest.embedding_runtime.clone())
        .unwrap_or_default();
    if model.info().provider == "local" {
        EmbeddingRuntimeConfig {
            device: Some(
                options
                    .embedding
                    .as_ref()
                    .map_or(current.device.unwrap_or(Device::Auto), |embedding| {
                        embedding.device
                    }),
            ),
            ..EmbeddingRuntimeConfig::default()
        }
    } else {
        EmbeddingRuntimeConfig {
            api_key: current.api_key,
            endpoint: options
                .embedding
                .as_ref()
                .and_then(|embedding| embedding.endpoint.clone())
                .or(current.endpoint)
                .or_else(|| model.info().endpoint.clone()),
            device: None,
        }
    }
}

fn embedding_schema(model: &ModelRuntimeLease) -> WorkspaceIndexEmbedding {
    let info = model.info();
    WorkspaceIndexEmbedding {
        provider: info.provider.clone(),
        model: info.name.clone(),
        dimension: info.dimension,
        metric: metric_name(info.metric).to_owned(),
    }
}

fn storage_embedding_schema(model: &ModelRuntimeLease) -> WorkspaceIndexEmbeddingSchema {
    let info = model.info();
    WorkspaceIndexEmbeddingSchema {
        provider: info.provider.clone(),
        model: info.name.clone(),
        dimension: info.dimension,
        metric: info.metric,
    }
}

const fn metric_name(metric: EmbeddingMetric) -> &'static str {
    match metric {
        EmbeddingMetric::Cosine => "cosine",
        EmbeddingMetric::DotProduct => "dot",
        EmbeddingMetric::Euclidean => "euclidean",
    }
}

fn is_indexed(manifest: &WorkspaceManifest) -> bool {
    manifest.index_policy == WorkspaceIndexPolicy::Enabled
        && manifest.embedding.is_some()
        && manifest.index_version.is_some()
}

fn workspace_index_location_from_option(
    root: Option<&Path>,
) -> Result<WorkspaceIndexLocation, EngineError> {
    workspace_index_location(&resolve_root(root)?)
}

fn resolve_root(root: Option<&Path>) -> Result<PathBuf, EngineError> {
    let root = root
        .map_or_else(env::current_dir, |root| Ok(root.to_path_buf()))
        .map_err(|error| {
            EngineError::backend("workspace", format!("resolve current directory: {error}"))
        })?;
    if root.is_absolute() {
        Ok(root)
    } else {
        env::current_dir()
            .map(|current| current.join(root))
            .map_err(|error| EngineError::backend("workspace", error.to_string()))
    }
}

fn default_root_path(root: &Path) -> RootPath {
    RootPath {
        path: root.to_path_buf(),
        recursive: true,
        discovery: DiscoveryOptions::default(),
    }
}

fn workspace_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace")
        .to_owned()
}

fn unindexed_info(location: WorkspaceIndexLocation, policy: WorkspaceIndexPolicy) -> InfoResult {
    InfoResult {
        root: location.root,
        indexed: false,
        index_policy: policy,
        home: location.home,
        index_path: location.index_path,
        source: InfoSource::Unindexed,
        workspace_index: None,
        status: None,
        suggestion: Some("run index to create a workspace index".to_owned()),
    }
}

fn workspace_suggestion(manifest: &WorkspaceManifest, indexed: bool) -> Option<String> {
    if manifest.index_policy == WorkspaceIndexPolicy::Disabled {
        Some("indexing is disabled for this workspace".to_owned())
    } else if !indexed {
        Some("workspace manifest exists but index storage is missing".to_owned())
    } else {
        None
    }
}

fn model_error(error: &ModelError) -> EngineError {
    let mut message = error.to_string();
    if let Some(code) = error.code() {
        message = format!("{code}: {message}");
    }
    if let Some(context) = error.context() {
        write!(&mut message, " ({context})").expect("writing to a String cannot fail");
    }
    EngineError::backend("models", message)
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    use tempfile::tempdir;

    use crate::{
        api::{
            index::{
                IndexOptions,
                options::{DiscoveryOptions, RootPath},
            },
            info::InfoOptions,
        },
        storage::spi::{
            FileIndexDiagnostics, FileInfo, IndexedFragment, ListEntitiesOptions, StorageResult,
            StorageSearchFilter, StorageSearchHit, StoredEntity, WorkspaceIndexStorage,
            WorkspaceIndexStorageFactory, WorkspaceIndexStorageOptions,
        },
    };

    use super::{ModelRuntimeManager, WorkspaceIndexService};

    #[derive(Debug, Default)]
    struct MemoryStorageFactory {
        exists: Arc<AtomicBool>,
    }

    impl WorkspaceIndexStorageFactory for MemoryStorageFactory {
        fn open(
            &self,
            options: WorkspaceIndexStorageOptions,
        ) -> StorageResult<Box<dyn WorkspaceIndexStorage>> {
            if !options.is_read_only() {
                self.exists.store(true, Ordering::Release);
            }
            Ok(Box::new(EmptyStorage {
                read_only: options.is_read_only(),
            }))
        }

        fn exists(&self, _storage_path: &Path) -> StorageResult<bool> {
            Ok(self.exists.load(Ordering::Acquire))
        }

        fn delete(&self, _storage_path: &Path) -> StorageResult<()> {
            self.exists.store(false, Ordering::Release);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct EmptyStorage {
        read_only: bool,
    }

    #[async_trait::async_trait]
    impl WorkspaceIndexStorage for EmptyStorage {
        fn is_read_only(&self) -> bool {
            self.read_only
        }

        fn get_file_by_path(&self, _absolute_path: &Path) -> StorageResult<Option<FileInfo>> {
            Ok(None)
        }

        fn list_files_by_path_prefix(&self, _absolute_path: &Path) -> StorageResult<Vec<FileInfo>> {
            Ok(Vec::new())
        }

        fn list_files_by_path_prefixes(
            &self,
            _absolute_paths: &[PathBuf],
        ) -> StorageResult<Vec<FileInfo>> {
            Ok(Vec::new())
        }

        fn list_files(&self) -> StorageResult<Vec<FileInfo>> {
            Ok(Vec::new())
        }

        fn list_entities_by_file(
            &self,
            _file_id: &str,
            _options: ListEntitiesOptions,
        ) -> StorageResult<Vec<StoredEntity>> {
            Ok(Vec::new())
        }

        fn get_entity(&self, _entity_id: &str) -> StorageResult<Option<StoredEntity>> {
            Ok(None)
        }

        fn search_fts(
            &self,
            _query: &str,
            _limit: usize,
            _filter: Option<&StorageSearchFilter>,
        ) -> StorageResult<Vec<StorageSearchHit>> {
            Ok(Vec::new())
        }

        fn search_vector(
            &self,
            _vector: &[f32],
            _limit: usize,
            _filter: Option<&StorageSearchFilter>,
        ) -> StorageResult<Vec<StorageSearchHit>> {
            Ok(Vec::new())
        }

        fn replace_file(
            &self,
            _file: &FileInfo,
            _entries: &[IndexedFragment],
            _diagnostics: Option<&FileIndexDiagnostics>,
        ) -> StorageResult<()> {
            Ok(())
        }

        fn mark_file_failed(&self, _file: &FileInfo, _error: &str) -> StorageResult<()> {
            Ok(())
        }

        fn delete_file(&self, _file_id: &str) -> StorageResult<()> {
            Ok(())
        }

        async fn finalize_writes(&self) -> StorageResult<()> {
            Ok(())
        }

        fn close(&self) -> StorageResult<()> {
            Ok(())
        }
    }

    #[test]
    fn dropping_a_missing_index_does_not_require_a_storage_backend() {
        let directory = tempdir().expect("temporary directory");

        assert!(
            !WorkspaceIndexService::new()
                .drop_index(&InfoOptions {
                    root: Some(directory.path().to_path_buf()),
                    include_status: false,
                })
                .expect("missing index should be an idempotent no-op")
        );
    }

    #[tokio::test]
    async fn composes_workspace_lifecycle_around_the_indexing_pipeline() {
        let directory = tempdir().expect("temporary directory");
        let sources = directory.path().join("sources");
        std::fs::create_dir(&sources).expect("source directory");
        let factory = Arc::new(MemoryStorageFactory::default());
        let service = WorkspaceIndexService::with_storage_factory(factory.clone());
        let models = ModelRuntimeManager::new();

        let result = service
            .index(
                &models,
                IndexOptions {
                    root: Some(directory.path().to_path_buf()),
                    roots: vec![RootPath {
                        path: sources,
                        recursive: true,
                        discovery: DiscoveryOptions::default(),
                    }],
                    ..IndexOptions::default()
                },
            )
            .await
            .expect("empty workspace should index");
        assert_eq!(result.files_scanned, 0);
        assert!(factory.exists.load(Ordering::Acquire));

        let info = service
            .info(InfoOptions {
                root: Some(directory.path().to_path_buf()),
                include_status: true,
            })
            .await
            .expect("workspace info");
        assert!(info.indexed);
        assert_eq!(info.status.expect("index status").files_stored, 0);

        let disabled = WorkspaceIndexService::disable_index(&InfoOptions {
            root: Some(directory.path().to_path_buf()),
            include_status: false,
        })
        .expect("disable index");
        assert!(!disabled.indexed);

        assert!(
            service
                .drop_index(&InfoOptions {
                    root: Some(directory.path().to_path_buf()),
                    include_status: false,
                })
                .expect("drop index")
        );
        assert!(!factory.exists.load(Ordering::Acquire));
        models.close();
    }
}
