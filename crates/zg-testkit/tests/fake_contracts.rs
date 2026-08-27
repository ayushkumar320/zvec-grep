use std::path::PathBuf;

use tempfile::tempdir;
use zg_engine::{EngineError, LexicalSearchRequest, ZvecGrep};
use zg_testkit::load_cli_case;

#[tokio::test]
async fn zvec_grep_executes_typed_requests_directly() {
    let temporary = tempdir().expect("temporary workspace should be created");
    let first_root = temporary.path().join("first");
    let second_root = temporary.path().join("second");
    std::fs::create_dir_all(&first_root).expect("first workspace should be created");
    std::fs::create_dir_all(&second_root).expect("second workspace should be created");
    std::fs::write(first_root.join("fixture.txt"), "first needle\n")
        .expect("first fixture should be written");
    std::fs::write(second_root.join("fixture.txt"), "second needle\n")
        .expect("second fixture should be written");
    let service = ZvecGrep::new();

    let first_reply = service
        .lexical_search(LexicalSearchRequest {
            root: Some(first_root.clone()),
            patterns: vec!["needle".to_owned()],
            ..LexicalSearchRequest::default()
        })
        .await
        .expect("first lexical request should complete");
    let second_reply = service
        .lexical_search(LexicalSearchRequest {
            root: Some(second_root.clone()),
            patterns: vec!["needle".to_owned()],
            ..LexicalSearchRequest::default()
        })
        .await
        .expect("second lexical request should complete");

    assert_eq!(first_reply.root, first_root);
    assert_eq!(first_reply.matches.len(), 1);
    assert_eq!(first_reply.matches[0].content, "first needle");
    assert_eq!(second_reply.root, second_root);
    assert_eq!(second_reply.matches.len(), 1);
    assert_eq!(second_reply.matches[0].content, "second needle");

    service.close();
    let error = service
        .lexical_search(LexicalSearchRequest {
            root: Some(temporary.path().to_path_buf()),
            patterns: vec!["needle".to_owned()],
            ..LexicalSearchRequest::default()
        })
        .await
        .expect_err("closed service must reject requests");
    assert!(matches!(error, EngineError::Closed));
}

#[test]
fn compatibility_fixture_schema_is_loadable() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/cli/managed-rg-no-match.json");
    let fixture = load_cli_case(&path).expect("fixture should be valid");
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.expected.exit_code, 0);
}
