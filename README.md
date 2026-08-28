# zvec-grep Rust rewrite

This branch contains the Rust implementation of zvec-grep. The application API
is centered on one reusable `ZvecGrep` engine value:

```rust,ignore
let zg = ZvecGrep::new();
let reply = zg.lexical_search(LexicalSearchRequest {
    root: Some(root.into()),
    ..request
}).await?;
zg.close();
```

`ZvecGrep` is normally shared for the lifetime of a process. Workspace root is
request state, so the same instance can serve multiple workspaces. It exposes
typed `query`, `lexical_search`, `index`, `inspect`,
`change_index` and `job` methods. It calls its private services directly; there
is no public command dispatcher, operation envelope, adapter registry or Core
layer between a method and its implementation.

The engine owns a private process-level model runtime manager. Workspaces using
the same model configuration share one runtime and its loaded weights/tokenizer;
embedding calls may execute concurrently against those shared resources.
`IndexRequest::on_progress` accepts an in-process `IndexProgressReporter` and
surfaces model downloads through `IndexProgress::embedding`. The reporter is
runtime-only and is omitted from serialized daemon requests.

Only `zg query --rg` is implemented end to end today. The other methods already
have their final typed shape and return `capability_unavailable` until their
service implementations land.

## Crates

- `zg-engine`: high-level requests, replies, errors and `ZvecGrep`; lexical
  search and embedding model implementations are private to this crate.
- `zg-cli`: CLI parsing and terminal rendering.
- `zg`: production binary.
- `zg-daemon`: process lifecycle, loopback HTTP server and stdio bootstrap.
- `zg-transport-mcp`: MCP schemas and direct translation to typed `ZvecGrep`
  calls.
- `zg-daemon-protocol`: daemon-only wire DTOs. These types are not part of the
  in-process engine API.
- `zg-host-native`: standalone native scanner and watcher implementation.
- `zg-testkit`: compatibility fixture readers.

Run the complete local gate with:

```sh
bash scripts/check.sh
```
