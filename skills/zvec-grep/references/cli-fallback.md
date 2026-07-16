# CLI fallback

Use these commands only when the native MCP tools are unavailable or when an explicit no-index lexical search is required.

## Choose the route

When the daemon is reachable but native MCP tools are unavailable in the current task, keep daemon ownership by using Server mode:

```bash
zg status --mode server
zg query "authentication flow" --fts "AuthService" -g "src/**" -g "!tests/**" -g "!dist/**" --mode server
```

Use Direct mode only when the daemon is unavailable or deliberately stopped. Do not perform Direct writes while a daemon lease is active.

## Check index state

Check status once per repository investigation and reuse it:

```bash
zg status --mode server
```

If the index is missing, do not build it automatically. Mention `zg index` and wait for the user to authorize persistent indexing.

## Search an existing index

Start with indexed hybrid search, then add exact or semantic routes as needed:

```bash
zg query "request validation" "error handling" -g "src/**" -g "!tests/**" -g "!dist/**" --limit 10 --preview short --mode server
zg query "authentication flow" --fts "AuthService" --fts "ForbiddenError" -g "src/**" --mode server
zg query --vector "where incoming requests are authorized" -g "src/**" --mode server
zg query "changes that must be indexed before search" --fresh --mode server
```

Server queries return the current index and refresh stale data in the background by default. Add `--fresh` only when the query must wait for pending index changes. Use `--preview none` for broad candidate scans, `--preview short` for a small deterministic window, and `--preview full` only after narrowing.

## Search without an index

Use managed ripgrep only for exhaustive literal or regex matching:

```bash
zg query --rg "SymbolName|LogMessage"
zg query --rg -F "ExactSymbolOrText" src
zg query --rg -i -C 2 -g "*.ts" -g "!dist/**" "needle text" src
```

Managed ripgrep always runs locally. It remains available while the daemon is running or an index writer is active; do not stop the daemon or switch client mode before using it.

Use `-e` or `--regexp` when the pattern begins with `-`. Common ripgrep flags include `-n`, `-H`, `-F`, `-i`, `-w`, `-A`, `-B`, `-C`, `-g`, `--hidden`, `--type`, `--type-not`, `--no-ignore`, `--smart-case`, and `--pcre2`.

## Index only with authorization

When the daemon is reachable, submit the index through Server mode with an explicit embedding model:

```bash
zg index --embedding local/embeddinggemma-300m --mode server
```

When the daemon is deliberately stopped, Direct mode can create a focused index:

```bash
zg index --embedding local/embeddinggemma-300m -g "src/**" -g "docs/**" -g "test/**" -g "!dist/**" -g "!node_modules/**" -g "!coverage/**" --mode direct
```

Rebuild only when an incompatible embedding schema or index version requires it. Existing indexes reuse their stored embedding schema unless the user intentionally changes it.
