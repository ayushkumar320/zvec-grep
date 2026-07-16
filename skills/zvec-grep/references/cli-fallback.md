# CLI fallback

Use these commands only when the native MCP tools are unavailable or when an explicit no-index lexical search is required.

## Choose the route

When the daemon is reachable but native MCP tools are unavailable in the current task, keep daemon ownership by using Server mode:

```bash
zg --mode server --status
zg --mode server "authentication flow" --fts "AuthService" --include "src/**" --exclude "tests/**,dist/**"
```

Use Direct mode only when the daemon is unavailable or deliberately stopped. Do not perform Direct writes while a daemon lease is active.

## Check index state

Check status once per repository investigation and reuse it:

```bash
zg --mode server --status
```

If the index is missing, do not build it automatically. Mention `zg --index` and wait for the user to authorize persistent indexing.

## Search an existing index

Start with indexed hybrid search, then add exact or semantic routes as needed:

```bash
zg --mode server "request validation" "error handling" --include "src/**" --exclude "tests/**,dist/**" --limit 10 --preview short
zg --mode server "authentication flow" --fts "AuthService" "ForbiddenError" --include "src/**"
zg --mode server --vector "where incoming requests are authorized" --include "src/**"
zg --mode server --fresh "changes that must be indexed before search"
```

Server queries return the current index and refresh stale data in the background by default. Add `--fresh` only when the query must wait for pending index changes. Use `--preview none` for broad candidate scans, `--preview short` for a small deterministic window, and `--preview full` only after narrowing.

## Search without an index

Use managed ripgrep only for exhaustive literal or regex matching:

```bash
zg --rg "SymbolName|LogMessage"
zg --rg -F "ExactSymbolOrText" src
zg --rg -i -C 2 -g "*.ts" -g "!dist/**" "needle text" src
```

Managed ripgrep always runs locally. It remains available while the daemon is running or an index writer is active; do not stop the daemon or switch client mode before using it.

Use `-e` or `--regexp` when the pattern begins with `-`. Common ripgrep flags include `-n`, `-H`, `-F`, `-i`, `-w`, `-A`, `-B`, `-C`, `-g`, `--hidden`, `--type`, `--type-not`, `--no-ignore`, `--smart-case`, and `--pcre2`.

## Index only with authorization

When the daemon is reachable, submit the index through Server mode with an explicit embedding model:

```bash
zg --mode server --index --embedding local/embeddinggemma-300m
```

When the daemon is deliberately stopped, Direct mode can create a focused index:

```bash
zg --mode direct --index --embedding local/embeddinggemma-300m --include "src/**,docs/**,test/**" --exclude "dist/**,node_modules/**,coverage/**,.zvec-grep/**"
```

Rebuild only when an incompatible embedding schema or index version requires it. Existing indexes reuse their stored embedding schema unless the user intentionally changes it.
