# Parallel-ready Rust architecture

## 1. Framework boundary

The outer Core seam is intentionally small:

```rust
Core::open(CoreConfig) -> Core
Core::run(Operation, RunControl) -> Outcome
Core::shutdown(deadline)
```

All Direct and daemon-backed entry points construct the same typed `Operation`
and consume the same `Outcome`. Transports translate owned DTOs; they do not
perform query normalization, ranking, freshness, authorization or model
selection. The one-method `OperationExecutor` seam returns transport-stable
`ErrorReply` values and is implemented by in-process `Core`,
`zg-testkit::ScriptedExecutor`, and the future daemon client.

In resident mode the daemon is the only process that owns a Core. MCP stdio is
a thin proxy:

```text
MCP stdio -> DaemonClient -> versioned local wire -> DaemonServer -> Core::run
```

`zg-daemon-protocol` owns only the versioned wire envelopes. Local IPC framing,
daemon process management and MCP framing remain transport adapters.

The current tracer bullet is executable:

```text
terminal argv
  -> typed LexicalSearch Operation
  -> shared Core lifecycle and resource admission
  -> spawned official rg --json
  -> canonical lexical reply
  -> terminal formatter
```

Other command envelopes are frozen enough for CLI/daemon/MCP mapping and return
a stable `capability_unavailable` result until their Core orchestration lands.

## 2. Typed commands

`Command` and `Reply` have matching exhaustive variants:

```text
Query          <-> QueryReply
LexicalSearch  <-> LexicalSearchReply
Index          <-> IndexReply
Inspect        <-> InspectReply
ChangeIndex    <-> ChangeIndexReply
Job            <-> JobReply
```

`Outcome` remains `Completed`, `Accepted` or `InputRequired`. Arbitrary JSON and
generic plugin registries are not accepted as shortcuts. Fields are derived
from the current TypeScript contracts; compatibility fixtures remain the source
of truth when a field needs refinement.
Transport failures use the serializable `ErrorReply` derived from stable
`CoreError` codes; transports do not invent their own error taxonomy.

Domain types are split by work locality under `zg-engine/src/domain/`. The
top-level operation envelope stays owned by the Core integrator so transport
and adapter branches do not each evolve a private protocol.

## 3. Internal ports

Core-owned coarse seams isolate native dependencies:

| Port | Responsibility | Test adapter |
| --- | --- | --- |
| `LexicalSearchPort` | exhaustive lexical search | `RecordedLexical` |
| `WorkspaceScannerPort` | metadata discovery and bounded source reads | `FixtureScanner` |
| `ExtractionPort` | batch document extraction | `FixtureExtraction` |
| `IndexStoragePort`/`IndexWritePort` | file state, recall and atomic generations | `InMemoryStorage` |
| `EmbeddingFactoryPort`/`EmbeddingSessionPort` | model load/embed/close | deterministic embedding |
| `ArtifactSourcePort` | verified local materialization | fixture artifact source |
| `ClockPort` | deterministic time | manual clock |
| `WorkspaceWatcherFactoryPort`/`WorkspaceWatchSessionPort` | daemon-owned normalized changes | manual watcher |

`CorePorts` accepts scanner, extraction, storage, embedding and clock
dependencies from the binary composition root. Model factories receive an
`ArtifactSourcePort` in their constructor. The daemon owns watch sessions and
turns each change batch into a normal Index operation. Adapter crates depend on
`zg-engine`; `zg-engine` never depends on a native adapter.

Each `RootSpec` is the only discovery-policy source consumed by scanner and
watcher adapters. `ScanRequest` additionally carries opaque known source
fingerprints so unchanged indexed files can avoid repeated binary sniffing.
Discovery returns kind and format hints plus bounded skip diagnostics; complete
file bytes are read only through `read_batch`. Watch changes use paths relative
to their `RootSpec` and preserve directory scope through `RescanDirectory` and
`DeletePrefix`.

Every external I/O seam has a fake or deterministic adapter in `zg-testkit`.
Storage, extraction, embedding, artifact and lexical also have reusable contract
suites. A production adapter calls the same suite from its integration tests.

## 4. Invariants

1. Direct invokes `Core::run`; MCP invokes the daemon through a client that
   implements the same `OperationExecutor` interface.
2. CLI `query --rg` stays local even when mode is Server, matching the current
   implementation, while still using the shared lexical Core path.
3. A lexical operation does not read a manifest, open zvec or load a model.
4. Adapters do not own normalization, ranking, freshness or authorization
   policy.
5. Result order is deterministic and independent of async completion order.
6. Event delivery is non-blocking; admitted operations attempt one started and
   one terminal event.
7. `shutdown` rejects new work and drains admitted operations up to its deadline.
8. CPU workers, blocking workers, concurrent operations, lexical processes and
   background jobs have explicit budgets. Adapters cannot create private
   unbounded runtimes or pools.
9. A storage writer publishes only through `finalize`; `ReplaceFile` removes all
   stale entities for that file and readers never observe pending mutations.
10. Native library, Clap, daemon framing and MCP types do not cross the Core seam.
11. Remote artifact or embedding work must complete authorization before any
   outbound request.
12. No Rust dynamic plugin ABI and no Node/Rust SDK are part of the product.
13. `RunControl` never crosses a process seam. The daemon wire carries remaining
    timeout, principal and trace; cancellation and events use separate frames.
14. Native file rename events are normalized to Delete/Upsert, directory
    changes retain RescanDirectory/DeletePrefix scope, and overflow or watcher
    recovery becomes Rescan before triggering Core operations.
15. Source fingerprints are adapter-owned opaque values. Core and storage may
    compare or persist them but must not parse their representation.

## 5. Crate policy and dependency direction

The framework contains only crates with an executable responsibility:

| Crate | Stable responsibility |
| --- | --- |
| `zg-engine` | Core, typed domain, lifecycle, errors, events, budgets and ports |
| `zg-daemon-protocol` | versioned daemon request/response/event envelopes |
| `zg-lexical-rg` | official ripgrep process and JSON adapter |
| `zg-host-native` | metadata-first scanning and normalized filesystem watch sessions |
| `zg-cli` | argument-to-Operation translation and terminal formatting |
| `zg-testkit` | fakes, fixture readers and reusable contract suites |
| `zg` | single binary and production composition root |

```text
                       zg-cli
                          |
zg-testkit -------> zg-engine <------- production adapters
                       ^  ^                    |
                       |  +-- zg-daemon-protocol
                       +---------- zg ---------+
                               composition root
```

Future owners add `zg-extract-native`, `zg-storage-zvec`, `zg-model-*`,
`zg-daemon` or `zg-transport-mcp` only with their first real
proof. The workspace member glob avoids central member-list edits. A crate
isolates a native dependency or independently testable policy seam; it does not
mirror a TypeScript class.

## 6. Parallel change protocol

Ownership and merge rules live in `OWNERS.md`; adapter setup is documented in
`crates/ADAPTER_GUIDE.md`.

- Port or command-envelope changes land as dedicated Core contract changes.
- Adapter work normally modifies only its crate and contract invocation.
- Daemon/MCP owners implement against `OperationExecutor` and the versioned
  protocol; they do not wait for storage or models.
- Production composition changes land separately after an adapter passes its
  shared contract.
- Compatibility changes require an oracle fixture and an allowed-difference ID.
- CI runs formatting, strict Clippy, all contracts and portable tests.

## 7. Compatibility data

`compat/` contains machine-readable TypeScript/Rust and Direct/Server contracts,
not Rust test implementations. The schema is versioned. Fixtures exclude
developer paths, secrets, random IDs and timings; the differential runner owns
normalization.

The first fixture records managed-rg no-match behavior. `mcp/`, `index/` and
`models/` are added with their first captured fixtures rather than as empty
directories.

## 8. Remaining product work

The framework is parallel-ready, but product parity is not complete. Independent
workstreams can now begin:

1. Capture TypeScript oracle cases and implement the differential runner.
2. Complete managed-rg flags, path security, structure enrichment and formatter
   parity in the lexical/CLI localities.
3. Integrate the proven native scanner into Core indexing and the watcher into
   daemon composition when those orchestration slices land.
4. Add extraction, zvec storage and model adapter crates with their first native
   proof and shared contract invocation.
5. Add the resident daemon client/server and MCP stdio thin proxy against the
   scripted executor.
6. Add model authorization, artifact verification, jobs and resident lifecycle
   orchestration inside the shared Core.
7. Add platform binaries and npm canary packaging after the binary contract is
   stable.
