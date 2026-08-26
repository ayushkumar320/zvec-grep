# Rust rewrite contributor guide

This guide is the starting point for contributors to the Rust rewrite. The
framework is ready for parallel work, but product parity is not complete. Pick
one workstream, stay inside its write locality, and verify behavior through the
shared Core or port interface.

## 1. Five-minute start

### Prerequisites

- `rustup`. The workspace pins Rust `1.98.0`, `rustfmt`, and Clippy in
  `rust-toolchain.toml`; do not use nightly-only features.
- A system `rg` executable for the current managed-ripgrep tracer bullet.
- Git. The standalone workspace is the root of the orphan `dev/rust` branch.

Node.js is not required to build or run the Rust POC. It is needed only by work
that captures results from the current TypeScript implementation or builds the
future npm distribution wrapper.

Clone the standalone branch into its own checkout:

```sh
git clone --branch dev/rust --single-branch \
  git@github.com:zvec-ai/zvec-grep.git zvec-grep-rust
cd zvec-grep-rust
rustup show active-toolchain
bash scripts/check.sh
cargo run -p zg -- --help
cargo run -p zg -- query --rg ivf-rabitq ..
```

The last command exercises the complete implemented slice:

```text
CLI arguments
  -> typed Operation
  -> Core resource admission and lifecycle
  -> official rg JSON adapter
  -> canonical Outcome
  -> terminal rendering
```

Only `zg query --rg` is implemented end to end today. Other typed commands are
present so transports and adapters can compile against stable shapes, but they
return `capability_unavailable` until their Core orchestration is implemented.

## 2. Read only what your workstream needs

Start with this file, then read the document for the interface you will use:

| Document | Read it when |
| --- | --- |
| `ARCHITECTURE.md` | changing Core behavior, a port, transport, lifecycle, or resource policy |
| `OWNERS.md` | claiming a workstream or deciding which files a branch may edit |
| `crates/ADAPTER_GUIDE.md` | adding an extraction, storage, model, artifact, or host adapter |
| `compat/README.md` | capturing TypeScript behavior or accepting a compatibility difference |
| `benchmarks/README.md` | adding a performance result or regression gate |

The main external Core interface is deliberately small:

```rust,ignore
Core::open(CoreConfig) -> Core
Core::run(Operation, RunControl) -> Outcome
Core::shutdown(deadline)
```

Callers and end-to-end tests use this same interface. Native dependencies vary
behind Core-owned ports, with production adapters and test adapters sharing the
same contract suites.

## 3. Pick a workstream

The detailed ownership table is in `OWNERS.md`. In normal work, one branch owns
one of these localities:

| Workstream | Normal write locality | Start by |
| --- | --- | --- |
| Contract and quality | `compat/`, `benchmarks/`, `zg-testkit` | capturing a TypeScript oracle fixture or extending a shared contract |
| Core integration | `crates/zg-engine`, composition policy in `crates/zg` | adding an in-memory end-to-end Core test |
| Lexical search | `crates/zg-lexical-rg` | adding a managed-rg parity case |
| Extraction | new `crates/zg-extract-native` | proving text extraction plus one real parser |
| Storage | new `crates/zg-storage-zvec` | reading a fixture index and atomically publishing one generation |
| Model runtime | one new `crates/zg-model-*` crate | producing a golden vector from one real model |
| HTTP or MCP transport | new `crates/zg-transport-*` crate | completing a successful flow with `ScriptedExecutor` |
| CLI and release | `crates/zg-cli`, packaging files | capturing one CLI case and matching its output and exit status |

Do not create placeholder crates. A new crate starts with its first real native
proof, its tests, and its shared contract invocation. Cargo discovers
`crates/*` automatically, so adding a crate does not require editing the root
workspace member list.

Before implementation, agree on a dedicated branch or worktree with the track
owner. A worktree may live beside the repository, but it must not become a
nested repository. Rebase or merge the latest Core contract before building an
adapter against it.

## 4. Rules that keep branches independent

1. Keep policy in `zg-engine`. CLI, HTTP, MCP, and native adapters translate
   data; they do not own normalization, ranking, freshness, authorization, or
   model-selection rules.
2. Keep native dependency types inside their adapter crate. Convert them to
   Core-owned domain types at the port seam.
3. Do not mix a port or command-envelope change with a production adapter
   implementation. Land the Core contract change first, then consume it.
4. Do not edit `zg/src/main.rs` from every adapter branch. Update the
   composition root separately after the adapter passes its contract.
5. Do not create private unbounded Tokio runtimes, Rayon pools, worker threads,
   subprocesses, queues, or background jobs. Use explicit Core resource budgets
   and make cancellation observable during expensive work.
6. Return deterministic ordering regardless of task completion order.
7. Add a seam only when behavior actually varies. Production and deterministic
   test adapters make a port real; a single pass-through implementation does
   not justify a new port.

The following files coordinate multiple tracks and should normally change only
in a dedicated Core integration branch:

- `crates/zg-engine/src/domain/operation.rs`
- `crates/zg-engine/src/config.rs`
- `crates/zg/src/main.rs`

If your task cannot proceed without editing one of them, first propose the
smallest interface change, its invariants, error behavior, and the adapters or
callers affected.

## 5. Adding a production adapter

Use `crates/ADAPTER_GUIDE.md` for the manifest template. The normal sequence is:

1. Identify the existing Core port and its observable contract. Do not expose
   native library types through the port.
2. Add one `zg-*` crate under `crates/` with a real dependency and a minimal
   working proof.
3. Depend on `zg-engine`; add `zg-testkit` as a development dependency.
4. Implement the port and map failures to stable `CoreError` codes.
5. Invoke the corresponding `zg_testkit::contracts::verify_*_contract` suite
   from the adapter's integration tests.
6. Add focused tests for native behavior that the shared contract cannot see,
   such as ABI compatibility or artifact fingerprints.
7. After the adapter is proven, integrate it into `zg` in a separate change.

Adapter implementations accept dependencies and return Core-owned results.
They must handle cancellation, deterministic output, explicit concurrency
limits, and cleanup of partially completed work.

## 6. Adding HTTP or MCP transport

Transport development must not wait for storage, extraction, or model runtimes.
Use `zg_testkit::ScriptedExecutor` as the adapter for the one-method
`OperationExecutor` interface:

1. Define versioned transport DTOs inside the transport crate.
2. Translate each request into the same typed `Operation` used by Direct mode.
3. Call `OperationExecutor` and translate `Outcome` or `ErrorReply` back to the
   protocol.
4. Test success, stable errors, cancellation, and shutdown against the scripted
   adapter.
5. Add equivalence tests that compare canonical Direct and Server results.

Transport-specific status codes and framing stay in the transport crate. The
transport must not invent a second error taxonomy or reimplement Core policy.

## 7. Direct and Server behavior

Direct, Server, and MCP must construct the same `Operation`, invoke the same
Core behavior, and consume the same canonical result. Compare canonical domain
results before terminal or protocol formatting rather than comparing incidental
logs or timing.

There is one intentional compatibility rule in the current tracer bullet:
`zg query --mode server --rg ...` still executes managed ripgrep locally, as the
TypeScript implementation does. It nevertheless passes through the same Core
lexical path as Direct mode.

Any new difference between Direct and Server modes requires all of the
following before implementation:

1. a captured fixture showing the behavior;
2. a stable ID in `compat/mode-differences.toml`;
3. a reason the modes cannot share the behavior;
4. a test that limits the difference to the documented case.

## 8. Capturing compatibility behavior

The current TypeScript implementation is the behavioral oracle during the
rewrite. For a compatibility change:

1. Reproduce the case against TypeScript first.
2. Store argv or request, stdout/result, stderr/error, and exit status in a
   schema-validated fixture under `compat/`.
3. Use repository-relative paths and stable placeholders. Never record a home
   directory, credential, random identifier, or wall-clock timing.
4. Normalize platform separators and volatile fields in the differential
   runner, not by repeatedly editing expected output.
5. Make the Rust test load the fixture through `zg-testkit`.

An intentional TypeScript/Rust difference needs an entry in
`compat/allowed-differences.toml`. A Direct/Server difference belongs in
`compat/mode-differences.toml`.

## 9. Local verification

Run the full local gate before requesting review:

```sh
bash scripts/check.sh
```

The script verifies formatting, each package independently, strict Clippy,
workspace tests, and Cargo metadata. Independent package checks matter because
workspace feature unification can otherwise hide a missing dependency feature.

Useful focused commands while iterating:

```sh
cargo test -p zg-engine
cargo test -p zg-testkit
cargo test -p zg-lexical-rg
cargo clippy -p zg-lexical-rg --all-targets -- -D warnings
RUST_LOG=debug cargo run -p zg -- query --mode server --rg needle ..
```

Do not weaken workspace lints to merge a branch. `unsafe` is forbidden in the
workspace; if a future native dependency requires unsafe integration, isolate
and review that policy explicitly instead of hiding it in an adapter change.

## 10. Ready-for-review checklist

A change is ready when all applicable items are true:

- The branch changes one workstream locality, or separates the Core contract
  and composition changes from the adapter implementation.
- New behavior is tested through the Core or port interface; tests do not rely
  on private implementation state.
- A production adapter passes the same shared contract as its test adapter.
- Native types, transport DTOs, Clap types, and model runtime types do not cross
  the Core seam.
- Errors use stable Core codes, output ordering is deterministic, and expensive
  work observes cancellation.
- Threads, processes, queues, and background jobs have explicit resource
  limits.
- TypeScript compatibility has a fixture; intentional differences have an
  allowed-difference ID.
- Direct and Server behavior is equivalent, except for a documented and tested
  entry in `compat/mode-differences.toml`.
- `bash scripts/check.sh` passes from a clean checkout of the branch.
- The pull request states the workstream, interface consumed, compatibility
  evidence, resource impact, and follow-up integration change if one remains.

## 11. Where to ask for an interface change

When an existing port or command cannot express required behavior, open a
small Core contract change before continuing the adapter branch. Include:

- the caller outcome that cannot currently be expressed;
- the proposed interface and its invariants;
- ordering, cancellation, resource, and error behavior;
- every production and test adapter affected;
- the compatibility fixture or new contract test;
- whether Direct and Server remain equivalent.

The goal is a deep Core module: callers learn a small, stable interface while
policy, lifecycle, and native orchestration remain local to its implementation.
