# CLI fallback

Use these commands only when an indexed MCP search tool is unavailable, failed,
or cannot perform the required operation, or when an authorized index lifecycle
or daemon-diagnostics task requires the CLI. Use native `grep` or `rg` for
no-index exact lookup.

## Choose the route

Leave `--mode` unset for ordinary status and query commands. The default Auto mode uses the daemon when reachable and Direct mode otherwise:

```bash
zg status
zg query "authentication flow" --fts "AuthService" -g "src/**" -g "!tests/**" -g "!dist/**"
```

Do not probe with `--mode server` and then retry with `--mode direct`. Use an explicit mode only when diagnosing routing or when the user requests one. Do not force Direct writes while a daemon lease is active.

## Check index state

Check status once per workspace investigation and reuse it:

```bash
zg status
```

If the index is missing, do not build it automatically. Mention `zg index` and wait for the user to authorize persistent indexing.

## Search an existing index

Start with indexed hybrid search, then add exact or semantic routes as needed:

```bash
zg query "request validation" "error handling" -g "src/**" -g "!tests/**" -g "!dist/**" --limit 10 --preview short
zg query "authentication flow" --fts "AuthService" --fts "ForbiddenError" -g "src/**"
zg query --vector "where incoming requests are authorized" -g "src/**"
zg query "changes that must be indexed before search" --refresh wait
```

When Auto selects Server, queries return the current index and refresh stale data in the background by default. Direct queries do not refresh by default. Add `--refresh wait` when the query must wait for pending index changes, or `--refresh off` to disable refresh explicitly. Use `--preview none` for broad candidate scans, `--preview short` for a small deterministic window, and `--preview full` only after narrowing.

## Recover from a Direct model failure

Direct mode already attempts CPU fallback when local GPU initialization fails. Do not repeat the query with an explicit CPU override.

If the local model or embedding context remains unavailable and exact anchors are available, remove positional hybrid and vector queries and search those anchors through the existing indexed FTS route. Indexed FTS does not require an embedding model:

```bash
zg query --fts "AuthService" --fts "ForbiddenError" -g "src/**" --limit 20 --preview short
```

Do not switch to managed ripgrep merely because semantic search is unavailable.
Use it only when the task requires exhaustive literal or regex matching and a
CLI fallback condition above is satisfied.

## Search exhaustively with managed ripgrep

Use managed ripgrep only for exhaustive literal or regex matching; it does not require an index:

```bash
zg query --rg "SymbolName|LogMessage"
zg query --rg -F "ExactSymbolOrText" src
zg query --rg -i -C 2 -g "*.ts" -g "!dist/**" "needle text" src
```

Managed ripgrep always runs locally. It remains available while the daemon is running or an index writer is active; do not stop the daemon or switch client mode before using it.

Use `-e` or `--regexp` when the pattern begins with `-`. Common ripgrep flags include `-n`, `-H`, `-F`, `-i`, `-w`, `-A`, `-B`, `-C`, `-g`, `--hidden`, `--type`, `--type-not`, `--no-ignore`, `--smart-case`, and `--pcre2`.

## Index only with authorization

When a server default model is known, let Auto submit the index without repeating the model:

```bash
zg index
```

Otherwise, use the embedding model selected by the user:

```bash
zg index --embedding local/potion-code-16m-v2
```

Use focused path filters for a new index:

```bash
zg index --embedding local/potion-code-16m-v2 -g "src/**" -g "docs/**" -g "test/**" -g "!dist/**" -g "!node_modules/**" -g "!coverage/**"
```

Rebuild only when an incompatible embedding schema or index version requires it. Existing indexes reuse their stored embedding schema unless the user intentionally changes it.
