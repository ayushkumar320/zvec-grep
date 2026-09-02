# zg-engine

`zg-engine` is the standalone, in-process library at the center of zvec-grep.
CLI, daemon, and MCP components consume its typed Rust API; they are not part
of the engine itself.

One long-lived `ZvecGrep` value may serve multiple workspace roots. It owns
process-level resources such as model runtimes, while workspace identity stays
in each request.

## Usage

```rust,ignore
use zg_engine::{
    EngineError,
    ZvecGrep,
    api::context::ContextOptions,
};

async fn search_workspace() -> Result<(), EngineError> {
    let engine = ZvecGrep::new();
    let result = engine
        .context(ContextOptions {
            root: Some("/workspace".into()),
            query: Some("workspace index".to_owned()),
            ..ContextOptions::default()
        })
        .await?;

    println!("{} results", result.items.len());
    engine.close();
    Ok(())
}
```

## Public API today

The crate root exposes:

- `ZvecGrep`, with typed `context`, `index`, `info`, and `drop_index` methods;
- `EngineError`, `EngineResult<T>`, and serializable `ErrorReport` diagnostics;
- the public `api` module.

Primary request and result types are grouped by operation:

- `api::context::{ContextOptions, ContextResult}`;
- `api::index::{IndexOptions, IndexResult}`;
- `api::info::{InfoOptions, InfoResult}`.

Secondary value types remain under the corresponding `options`, `result`, or
`progress` module. All other top-level engine modules are private implementation
details. A `pub` item nested inside one of those private modules is not part of
the crate's externally reachable API.

The engine API describes library operations. CLI arguments, daemon envelopes,
transport dispatch, and backend registries belong outside this crate.

## Error contract

`EngineError` exposes ten stable string codes:

- `ZG.ENGINE.INVALID_ARGUMENT`
- `ZG.ENGINE.NOT_FOUND`
- `ZG.ENGINE.UNSUPPORTED`
- `ZG.ENGINE.PERMISSION_DENIED`
- `ZG.ENGINE.RESOURCE_BUSY`
- `ZG.ENGINE.RESOURCE_CLOSED`
- `ZG.ENGINE.STORAGE_FAILURE`
- `ZG.ENGINE.CANCELLED`
- `ZG.ENGINE.DEADLINE_EXCEEDED`
- `ZG.ENGINE.INTERNAL`

`EngineError` displays only its readable message. Use `error.code()` for
programmatic handling and `error.report()` when logging or crossing a transport
boundary; the report also carries help text and source locations. The
`error.is_retryable()` policy is shared by transports and returns `true` only
for resource contention and exceeded deadlines.

## Current module boundaries

In this document, a self-contained module owns a coherent capability and does
not depend on another workflow or on that workflow's request/result DTOs.
Depending on shared error and domain values does not break that boundary.

| Module | Role | Current boundary |
| --- | --- | --- |
| `api` | Public requests, results, diagnostics, and progress values | Self-contained public contract; depends only on standard/serialization types and other API values |
| `error` | Stable engine error surface | Self-contained; private module with `EngineError`, `EngineResult<T>`, `ErrorReport`, and `ErrorSite` re-exported at the crate root |
| `payload` | Internal text and image content values | Self-contained private value module |
| `lexical` | Embedded grep retrieval | The retrieval core is self-contained; structure enrichment currently couples it to `api` and `extraction` |
| `extraction` | Text, Markdown, code, and image conversion into fragments | Cohesive, but currently depends on context API value types and `payload` |
| `models` | Embedding catalog, runtimes, and concrete model backends | Cohesive, but currently depends on index API device/progress types and `payload` |
| `workspace` | Index layout, manifest, and locking | Partially self-contained; manifest uses API values and layout currently coordinates storage reset |
| `storage` | Persistence contract shared by indexing and search | Internal shared SPI, not self-contained; its records currently use values from several modules |
| `indexing` | Scan, diff, extraction, embedding, and commit workflow | Orchestration layer by design |
| `search` | Request planning, indexed retrieval, fusion, and result assembly | Orchestration and algorithm layer by design |
| `service` | `ZvecGrep` composition, lifecycle, and operation routing | Top-level application coordinator by design |

This means the whole crate is a standalone library, but its private capability
modules are not independently supported libraries or public extension points.

## Target dependency shape

The rewrite is moving toward the following dependency direction:

```text
CLI / daemon / MCP
        |
        v
     ZvecGrep
        |
        v
 indexing / search workflows
        |
        v
 workspace / source / extraction / embedding / index store
        |
        v
 private native and third-party adapters
```

The intended boundary rules are:

- only the facade, operation types, errors, and stable caller-owned values are
  public;
- indexing and search are sibling workflows and do not invoke one another;
- lower-level capabilities depend on neutral domain values, not public
  operation DTOs;
- only the engine composition layer selects and wires concrete backends;
- scanner, extractor, embedding, and index-store contracts remain crate-private
  unless an external plugin API becomes an explicit product requirement;
- no generic command bus, global adapter registry, or public backend SPI is
  introduced.

## Development

Run commands from the repository root:

```sh
cargo check -p zg-engine --all-targets
cargo test -p zg-engine
RUSTDOCFLAGS="-D warnings" cargo doc -p zg-engine --no-deps
```

See the [repository overview](../../README.md),
[contributor guide](../../CONTRIBUTING.md), and
[`zg-host-native`](../zg-host-native/README.md) for surrounding components.
