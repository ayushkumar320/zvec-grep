# zvec-grep Rust rewrite

This orphan branch is the standalone clean-room Rust implementation of
`zvec-grep`. Its Cargo workspace starts at the branch root and its Git history
is intentionally independent from the TypeScript implementation on `main`.

New contributors should start with [`CONTRIBUTING.md`](CONTRIBUTING.md). It
covers environment setup, the first runnable command, workstream ownership,
adapter and transport workflows, compatibility rules, and the review gate.

The framework is ready for independent adapter, transport, compatibility and
release workstreams. The first executable tracer bullet remains deliberately
narrow:

```text
zg query --rg
  -> zg-cli (parse and render only)
  -> zg-engine::Core::run(Operation)
  -> zg-lexical-rg (official rg JSON adapter)
  -> canonical Core reply
```

Even when `--mode server` is selected, CLI `--rg` remains local for compatibility
with the current implementation. Future Direct, daemon-backed and MCP entry
points must all construct the same typed `Operation` and use the same Core
execution contract.

## Workspace now

- `zg-engine`: the deep Core module, owned domain types, lifecycle, resource
  budget, errors, events and internal ports.
- `zg-daemon-protocol`: versioned Execute/Cancel/Event envelopes shared by the
  resident daemon and thin clients.
- `zg-lexical-rg`: the first production adapter. It owns process invocation and
  ripgrep JSON decoding.
- `zg-cli`: Clap arguments, translation into an `Operation`, and terminal
  formatting. It contains no search policy.
- `zg-testkit`: deterministic fakes, compatibility fixture readers and shared
  adapter contract suites.
- `zg`: the only binary and composition root.

Core-owned command envelopes cover Query, LexicalSearch, Index, Inspect,
ChangeIndex and Job. Native seams cover lexical, metadata-first scanning,
extraction, file-level storage generations, embedding, verified artifact
materialization, clock and daemon-owned watch sessions. New production adapter
crates are added only when their first real proof starts. Planned names are
`zg-extract-native`, `zg-storage-zvec`, `zg-model-*`, `zg-host-native`,
`zg-daemon`, and `zg-transport-mcp`. The workspace uses `crates/*`, so a new
owner can add a crate without editing the root member list.

## Dependency rule

```text
                    zg-cli
                       |
zg-testkit ----> zg-engine <---- zg-lexical-rg
                       ^                 |
                       +------- zg ------+
                            composition root
```

`zg-engine` must not depend on Clap, Axum, rmcp, zvec, ORT, llama.cpp or a model
runtime. Transport DTOs and native dependency types are translated at adapter
boundaries.

## Local checks

```sh
scripts/check.sh
cargo run -p zg -- query --rg ivf-rabitq ..
```

The POC invokes a system `rg`. Bundling the platform ripgrep binary belongs to
the npm/release slice and does not change the Core interface.

## Ownership during parallel development

| Locality | Owner responsibility |
| --- | --- |
| `zg-engine` | Frozen `open/run/shutdown` interface, domain contracts, policy and integration |
| Each adapter crate | One native dependency and its contract implementation |
| `zg-cli`, `zg`, packaging | CLI compatibility, composition, binary and npm platform packages |
| `zg-testkit`, `compat`, `benchmarks` | Oracle fixtures, differential tests, quality and performance gates |

Changes to Core-owned request, reply, error or event types land separately
before adapter changes. Production adapters and fakes must pass the same
contract suite.

See `CONTRIBUTING.md` for the complete startup path, `ARCHITECTURE.md` for the
frozen seams, `OWNERS.md` for exclusive work localities,
`crates/ADAPTER_GUIDE.md` for adding an adapter, and `compat/README.md` for
oracle fixture rules.
