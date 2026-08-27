# Parallel work ownership

The purpose of ownership is locality: normal implementation work stays inside
one workstream. Ownership is not permission to fork Core behavior.

| Track | Exclusive write locality | Shared interface consumed | First production proof |
| --- | --- | --- | --- |
| Contract/quality | `compat/`, `benchmarks/`, `zg-testkit` | all Core and port interfaces | TypeScript oracle capture and differential runner |
| Core integrator | `zg-engine`, `zg` composition policy | `open/run/shutdown`, command envelopes | in-memory end-to-end operation suites |
| Lexical | `zg-lexical-rg` | `LexicalSearchPort` | managed-rg parity |
| Extraction | new `zg-extract-native` | `ExtractionPort` | text plus one tree-sitter grammar |
| Host | `zg-host-native` | scanner and watch factory/session ports | metadata scan plus normalized change batch |
| Storage | new `zg-storage-zvec` | `IndexStoragePort`, `IndexWritePort` | file state, replacement and atomic fixture generation |
| Model base | new `zg-model-model2vec`, artifact implementation | embedding/artifact ports | default model golden vector |
| ONNX | new `zg-model-onnx` | embedding/artifact ports | one ONNX model golden vector |
| GGUF | new `zg-model-llama` | embedding/artifact ports | one GGUF model golden vector |
| Runtime | `zg-daemon`, `zg-transport-mcp` | daemon protocol, typed `Operation/Outcome`, `OperationExecutor` | agent/full HTTP MCP lifecycle and concurrency-safe stdio thin proxy |
| CLI/release | `zg-cli`, platform/npm packaging | typed `Operation/Outcome` | native CLI parity and package canary |

## Change protocol

1. A port or command-envelope change is a dedicated Core contract change. Do
   not mix it into a production adapter implementation.
2. Core integrator and contract/quality owner review that change before adapter
   branches consume it.
3. An adapter owner normally changes only its crate and adds a call to the
   corresponding `zg-testkit::contracts::verify_*_contract` function.
4. Daemon/MCP owners use `ScriptedExecutor`. The daemon is the sole resident
   Core owner; MCP stdio remains a thin proxy and neither reproduces Core policy.
5. The `zg` composition root is updated by an integration change after the
   adapter contract passes; adapter branches do not each invent composition.
6. Direct/Server behavior differences require a machine-readable entry under
   `compat/` before implementation.

## Parallel-ready definition

A workstream is ready to claim when:

- its Core-owned request/reply or port types compile;
- a fake adapter exists when the seam needs substitution;
- a reusable contract suite exists or is added in the same interface change;
- production work has an exclusive crate/path;
- the owner can run `scripts/check.sh` without another workstream;
- normal implementation does not require editing `zg-engine/src/domain/operation.rs`,
  `zg-engine/src/config.rs`, or `zg/src/main.rs`.
