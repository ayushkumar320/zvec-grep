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

The resident daemon and public MCP endpoint are also runnable:

```sh
cargo run -p zg -- server on --mcp-toolset agent
# Or expose lifecycle/admin tools as well:
cargo run -p zg -- server on --mcp-toolset full
cargo run -p zg -- server status
cargo run -p zg -- server off
```

`server on` starts the same `zg` binary in the background and reports a
loopback Streamable HTTP URL, normally `http://127.0.0.1:7999/mcp`. The default
`agent` toolset exposes only `zvec_grep_search`. The opt-in `full` toolset also
exposes index, index-drop, index-status, managed-rg and daemon-status tools.
Every workspace tool validates its input and translates it into the same typed
Core operation used by other entry points. Managed-rg and daemon-status are
runnable now; Query, Index and Inspect return the stable
`capability_unavailable` error until their Core orchestration lands.

## Workspace now

- `zg-engine`: the deep Core module, owned domain types, lifecycle, resource
  budget, errors, events and internal ports.
- `zg-daemon-protocol`: versioned Execute/Cancel/Event envelopes shared by the
  resident daemon and thin clients.
- `zg-transport-mcp`: agent/full MCP schemas, request translation, managed-rg
  command safety and compact result formatting around `OperationExecutor`.
- `zg-daemon`: loopback Streamable HTTP hosting, instance records, health,
  controlled background process lifecycle and resident per-root watcher loops.
- `zg-lexical-rg`: the first production adapter. It owns process invocation and
  ripgrep JSON decoding.
- `zg-host-native`: the production metadata scanner and daemon-owned filesystem
  watcher. Scanner and watcher share one discovery-policy implementation.
- `zg-cli`: Clap arguments, translation into an `Operation`, and terminal
  formatting. It contains no search policy.
- `zg-testkit`: deterministic fakes, compatibility fixture readers and shared
  adapter contract suites.
- `zg`: the only binary and composition root.

Core-owned command envelopes cover Query, LexicalSearch, Index, Inspect,
ChangeIndex and Job. Native seams cover lexical, metadata-first scanning,
extraction, file-level storage generations, embedding, verified artifact
materialization, clock and daemon-owned watch sessions. Scanner results retain
format hints and bounded skip diagnostics; watcher batches preserve scoped
directory rescans and deletions all the way into incremental Index operations.
After a synchronous daemon Index succeeds, its root watcher is attached once;
Drop/Disable and daemon shutdown stop it. New production adapter crates are
added only when their first real proof starts. Planned names are
`zg-extract-native`, `zg-storage-zvec`, and `zg-model-*`. The workspace uses
`crates/*`, so a new
owner can add a crate without editing the root member list.

## Dependency rule

```text
                    zg-cli
                       |
zg-testkit ----> zg-engine <---- zg-lexical-rg
                       ^  ^               |
                       |  +---- zg-host-native
                       +---- zg-transport-mcp <-- zg-daemon
                                      ^             |
                                      +----- zg ----+
                                          composition root
```

`zg-engine` must not depend on Clap, Axum, rmcp, zvec, ORT, llama.cpp or a model
runtime. Transport DTOs and native dependency types are translated at adapter
boundaries.

## Local checks

```sh
scripts/check.sh
cargo run -p zg -- query --rg ivf-rabitq ..
cargo run -p zg -- server on --mcp-toolset agent
cargo run -p zg -- server off
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
