# Adding a production adapter

Create the crate only when it contains a real native proof. Do not add an empty
pass-through crate.

Example manifest shape:

```toml
[package]
name = "zg-extract-native"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[dependencies]
async-trait.workspace = true
zg-engine = { path = "../zg-engine" }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt"] }
zg-testkit = { path = "../zg-testkit" }

[lints]
workspace = true
```

The first adapter test calls the shared contract rather than duplicating it:

```rust,no_run
# async fn example(adapter: &dyn zg_engine::ExtractionPort) {
zg_testkit::contracts::verify_extraction_contract(adapter)
    .await
    .expect("native extraction must satisfy the Core contract");
# }
```

Checklist:

- Native/library types remain inside the adapter crate.
- Errors are mapped to stable `CoreError` codes.
- Cancellation is observed before and during expensive work.
- Output order is deterministic.
- No private unbounded Tokio or Rayon pool is created.
- Contract tests pass with the production adapter.
- Native revision, ABI and artifact fingerprints are recorded where applicable.
- The composition root is changed separately after the adapter is proven.
