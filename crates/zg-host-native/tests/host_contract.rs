use std::{error::Error, fs};

use tempfile::tempdir;
use zg_engine::{DiscoveryOptions, RootSpec, ScanRequest, WatchRequest};
use zg_host_native::{NativeScanner, NativeWatcherFactory};
use zg_testkit::contracts::{verify_scanner_contract, verify_watcher_lifecycle_contract};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[tokio::test]
async fn native_host_adapters_satisfy_shared_contracts() -> TestResult {
    let temporary = tempdir()?;
    fs::write(temporary.path().join("contract.rs"), "fn contract() {}\n")?;
    let root = RootSpec {
        path: temporary.path().to_path_buf(),
        recursive: true,
        discovery: DiscoveryOptions::default(),
    };

    verify_scanner_contract(
        &NativeScanner::default(),
        &ScanRequest {
            roots: vec![root.clone()],
            known_files: Vec::new(),
        },
    )
    .await?;
    verify_watcher_lifecycle_contract(&NativeWatcherFactory::default(), &WatchRequest { root })
        .await?;
    Ok(())
}
