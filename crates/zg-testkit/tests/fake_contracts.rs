use std::{path::PathBuf, sync::Arc, time::Duration};

use tokio_util::sync::CancellationToken;
use zg_engine::{
    ArtifactRequest, Command, Core, CoreConfig, CoreError, CoreEventKind, CorePorts, Device,
    DiscoveryOptions, EmbeddingModelSpec, ErrorCode, FileKind, JobReceipt, KnownSourceFile,
    LexicalSearchRequest, MaterializedArtifact, Operation, OperationExecutor, Outcome,
    QueryRequest, ReadBatchRequest, RootSpec, RunControl, ScanRequest, SourceFile, WatchRequest,
    WorkspaceChange, WorkspaceChangeBatch, WorkspaceScannerPort, WorkspaceWatcherFactoryPort,
};
use zg_testkit::{
    contracts::{
        verify_artifact_source_contract, verify_embedding_contract, verify_extraction_contract,
        verify_lexical_contract, verify_storage_contract,
    },
    fakes::{
        DeterministicEmbeddingFactory, FixtureArtifactSource, FixtureExtraction, FixtureScanner,
        InMemoryStorage, ManualWatcher, RecordedEvents, RecordedLexical, ScriptedExecutor,
    },
    load_cli_case,
};

#[tokio::test]
async fn fake_adapters_satisfy_shared_contracts() {
    let lexical = RecordedLexical::default();
    verify_lexical_contract(&lexical, std::path::Path::new("/workspace"))
        .await
        .expect("lexical fake must satisfy its contract");

    verify_extraction_contract(&FixtureExtraction::default())
        .await
        .expect("extraction fake must satisfy its contract");

    verify_embedding_contract(&DeterministicEmbeddingFactory::new(8))
        .await
        .expect("embedding fake must satisfy its contract");

    verify_storage_contract(
        &InMemoryStorage::default(),
        std::path::Path::new("/workspace"),
    )
    .await
    .expect("storage fake must satisfy its contract");

    let artifacts = FixtureArtifactSource::default();
    let expected = MaterializedArtifact {
        path: PathBuf::from("/fixtures/model.bin"),
        resolved_revision: "v1".to_owned(),
        sha256: "fixture-sha256".to_owned(),
        cache_hit: true,
    };
    artifacts.insert("fixture/model", expected.clone());
    verify_artifact_source_contract(
        &artifacts,
        &ArtifactRequest {
            reference: "fixture/model".to_owned(),
            revision: Some("v1".to_owned()),
            expected_sha256: Some("fixture-sha256".to_owned()),
            cache_dir: PathBuf::from("/fixtures"),
        },
        &expected,
    )
    .await
    .expect("artifact fake must satisfy its contract");
}

#[tokio::test]
async fn core_exposes_registered_capabilities_and_stable_missing_capability_errors() {
    let lexical = Arc::new(RecordedLexical::default());
    let scanner = Arc::new(FixtureScanner::default());
    let core = Core::open(CoreConfig::new(
        CorePorts::new()
            .with_lexical(lexical.clone())
            .with_scanner(scanner),
    ))
    .await
    .expect("Core should open");
    assert_eq!(core.capabilities(), ["lexical_search", "scanner"]);

    let events = Arc::new(RecordedEvents::default());
    let mut control = RunControl::local(CancellationToken::new());
    control.events = events.clone();
    core.run(
        Operation::lexical(
            PathBuf::from("/workspace"),
            LexicalSearchRequest {
                patterns: vec!["needle".to_owned()],
                ..LexicalSearchRequest::default()
            },
        ),
        control,
    )
    .await
    .expect("registered lexical operation should complete");
    assert_eq!(lexical.requests().len(), 1);
    let events = events.events();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0].kind, CoreEventKind::Started));
    assert!(matches!(events[1].kind, CoreEventKind::Completed { .. }));
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);

    let error = core
        .run(
            Operation::new(
                PathBuf::from("/workspace"),
                Command::Query(QueryRequest::default()),
            ),
            RunControl::local(CancellationToken::new()),
        )
        .await
        .expect_err("unimplemented query must return a stable capability error");
    assert!(matches!(
        error,
        CoreError::CapabilityUnavailable { capability } if capability == "query"
    ));

    core.shutdown(Duration::from_secs(1))
        .await
        .expect("Core should shut down");
}

#[test]
fn compatibility_fixture_schema_is_loadable() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/cli/managed-rg-no-match.json");
    let fixture = load_cli_case(&path).expect("fixture should be valid");
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.expected.exit_code, 0);
}

#[test]
fn all_current_command_envelopes_serialize() {
    let operation = Operation::new(
        PathBuf::from("/workspace"),
        Command::Query(QueryRequest::default()),
    );
    let encoded = serde_json::to_string(&operation).expect("operation should serialize");
    assert!(encoded.contains("\"kind\":\"query\""));

    let model = EmbeddingModelSpec {
        reference: "fixture/model".to_owned(),
        revision: None,
        cache_dir: None,
        endpoint: None,
        device: Device::Cpu,
    };
    assert_eq!(model.reference, "fixture/model");
}

#[tokio::test]
async fn scripted_executor_unblocks_transport_work_without_real_native_adapters() {
    let executor = ScriptedExecutor::default();
    executor.respond(
        "query",
        Outcome::Accepted(JobReceipt {
            id: "job-1".to_owned(),
        }),
    );
    let outcome = executor
        .execute(
            Operation::new(
                PathBuf::from("/workspace"),
                Command::Query(QueryRequest::default()),
            ),
            RunControl::local(CancellationToken::new()),
        )
        .await
        .expect("scripted transport execution should complete");
    assert!(matches!(outcome, Outcome::Accepted(_)));
    assert_eq!(executor.operations().len(), 1);

    let error = executor
        .execute(
            Operation::new(
                PathBuf::from("/workspace"),
                Command::Index(zg_engine::IndexRequest::default()),
            ),
            RunControl::local(CancellationToken::new()),
        )
        .await
        .expect_err("missing scripted reply must use a stable transport error");
    assert_eq!(error.code, ErrorCode::CapabilityUnavailable);
}

#[tokio::test]
async fn host_interfaces_support_metadata_first_scans_and_resident_watch_sessions() {
    let root = PathBuf::from("/workspace");
    let scanner = FixtureScanner::default();
    scanner.insert(SourceFile {
        root: root.clone(),
        relative_path: PathBuf::from("src/lib.rs"),
        bytes: b"fn main() {}".to_vec(),
        source_fingerprint: "source-v1".to_owned(),
        kind_hint: Some(FileKind::Code),
        format_hint: Some("rust".to_owned()),
    });
    let control = RunControl::local(CancellationToken::new());
    let request = ScanRequest {
        roots: vec![RootSpec {
            path: root.clone(),
            recursive: true,
            discovery: DiscoveryOptions::default(),
        }],
        known_files: vec![KnownSourceFile {
            root: root.clone(),
            relative_path: PathBuf::from("src/lib.rs"),
            source_fingerprint: "source-v1".to_owned(),
        }],
    };
    let snapshot = scanner
        .discover(&request, &control)
        .await
        .expect("fixture scan should discover metadata");
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.files[0].source_fingerprint, "source-v1");
    assert_eq!(snapshot.files[0].format_hint.as_deref(), Some("rust"));
    assert_eq!(snapshot.diagnostics.skipped_files, 0);
    let sources = scanner
        .read_batch(
            &ReadBatchRequest {
                files: snapshot.files,
            },
            &control,
        )
        .await
        .expect("fixture scanner should read requested source bytes");
    assert_eq!(sources[0].bytes, b"fn main() {}");

    let watcher = ManualWatcher::default();
    let watch_request = WatchRequest {
        root: RootSpec {
            path: root,
            recursive: true,
            discovery: DiscoveryOptions::default(),
        },
    };
    let session = watcher
        .watch(&watch_request, &control)
        .await
        .expect("watch session should open");
    watcher.push(WorkspaceChangeBatch {
        changes: vec![
            WorkspaceChange::Upsert(PathBuf::from("src/lib.rs")),
            WorkspaceChange::RescanDirectory(PathBuf::from("docs")),
            WorkspaceChange::DeletePrefix(PathBuf::from("generated")),
        ],
    });
    let changes = session
        .next_changes(&control)
        .await
        .expect("watch session should yield a normalized batch");
    assert_eq!(changes.changes.len(), 3);
    assert_eq!(watcher.requests(), [watch_request]);
    session.close().await.expect("watch session should close");
    let error = session
        .next_changes(&control)
        .await
        .expect_err("closed session must reject further reads");
    assert!(matches!(error, CoreError::ShuttingDown));
}
