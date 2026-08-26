# Parallel-ready Rust architecture

## 1. Framework boundary

The outer Core seam is intentionally small:

```rust
Core::open(CoreConfig) -> Core
Core::run(Operation, RunControl) -> Outcome
Core::shutdown(deadline)
```

All Direct, HTTP and MCP entry points construct the same typed `Operation` and
consume the same `Outcome`. Transports translate owned DTOs; they do not perform
query normalization, ranking, freshness, authorization or model selection.
The one-method `OperationExecutor` seam is implemented by in-process `Core` and
`zg-testkit::ScriptedExecutor`; loopback HTTP becomes a third adapter. This lets
CLI/HTTP/MCP owners implement successful protocol flows before native storage or
models exist.

The current tracer bullet is executable:

```text
terminal argv
  -> typed LexicalSearch Operation
  -> shared Core lifecycle and resource admission
  -> spawned official rg --json
  -> canonical lexical reply
  -> terminal formatter
```

Other command envelopes are frozen enough for CLI/HTTP/MCP mapping and return a
stable `capability_unavailable` result until their Core orchestration lands.

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
| `ExtractionPort` | batch document extraction | `FixtureExtraction` |
| `IndexStoragePort`/`IndexWritePort` | recall and atomic generations | `InMemoryStorage` |
| `EmbeddingFactoryPort`/`EmbeddingSessionPort` | model load/embed/close | deterministic embedding |
| `ArtifactSourcePort` | verified local materialization | fixture artifact source |
| `ClockPort` | deterministic time | manual clock |
| `WorkspaceWatcherPort` | coalesced change batches | manual watcher |

`CorePorts` accepts these dependencies from the binary composition root. A
missing production capability is explicit and machine-readable. Adapter crates
depend on `zg-engine`; `zg-engine` never depends on a native adapter.

Every external I/O seam has a fake or deterministic adapter in `zg-testkit`.
Storage, extraction, embedding, artifact and lexical also have reusable contract
suites. A production adapter calls the same suite from its integration tests.

## 4. Invariants

1. Direct, Server and MCP invoke the same `Core::run`.
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
9. A storage writer publishes only through `finalize`; readers never observe
   pending mutations or partial rebuilds.
10. Native library, Clap, HTTP and MCP types do not cross the Core seam.
11. Remote artifact or embedding work must complete authorization before any
   outbound request.
12. No Rust dynamic plugin ABI and no Node/Rust SDK are part of the product.

## 5. Crate policy and dependency direction

The framework contains only crates with an executable responsibility:

| Crate | Stable responsibility |
| --- | --- |
| `zg-engine` | Core, typed domain, lifecycle, errors, events, budgets and ports |
| `zg-lexical-rg` | official ripgrep process and JSON adapter |
| `zg-cli` | argument-to-Operation translation and terminal formatting |
| `zg-testkit` | fakes, fixture readers and reusable contract suites |
| `zg` | single binary and production composition root |

```text
                    zg-cli
                       |
zg-testkit ----> zg-engine <---- production adapters
                       ^                 |
                       +------- zg ------+
                            composition root
```

Future owners add `zg-extract-native`, `zg-storage-zvec`, `zg-model-*`,
`zg-transport-http` or `zg-transport-mcp` only with their first real proof. The
workspace member glob avoids central member-list edits. A crate isolates a
native dependency or independently testable policy seam; it does not mirror a
TypeScript class.

## 6. Parallel change protocol

Ownership and merge rules live in `OWNERS.md`; adapter setup is documented in
`crates/ADAPTER_GUIDE.md`.

- Port or command-envelope changes land as dedicated Core contract changes.
- Adapter work normally modifies only its crate and contract invocation.
- HTTP/MCP owners work with fake adapters and do not wait for storage/models.
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
3. Add extraction, zvec storage and model adapter crates with their first native
   proof and shared contract invocation.
4. Add loopback HTTP and MCP adapters against fake Core dependencies.
5. Add model authorization, artifact verification, jobs and resident lifecycle
   orchestration inside the shared Core.
6. Add platform binaries and npm canary packaging after the binary contract is
   stable.
