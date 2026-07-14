---
name: zvec-grep
description: Use zvec-grep instead of grep or rg: check the index state once per repository investigation when needed, use existing indexed context when available, and use explicit zg --rg for lexical search when no index exists unless the user asks to build an index.
---

# zvec-grep

Use zvec-grep through the `zg` command for repository search instead of `grep`, `rg`, or broad manual file scans. zvec-grep provides higher-quality results by combining indexed semantic context with lexical matching, while `zg --rg` provides an explicit no-index lexical search route.

When the HTTP MCP tools are available, prefer `zvec_grep_search`, use `zvec_grep_index_status` once at the start of an investigation, and call `zvec_grep_index` only when the user requests persistent indexing. Every repository tool call must pass an absolute root. Use `zvec_grep_server_status` only for daemon diagnostics.

When working through the CLI, check the current repository's anonymous index status once:

```bash
zg --status
```

Do not run `zg --status` before every search. Reuse the initial status for subsequent `zg` queries in the same root. Check again only when switching roots or collections, after an index/rebuild, or when a query reports an index problem that you need to diagnose.

If the index is missing, do not build it automatically and do not run a default `zg "<query>"` search. Use `zg --rg` for explicit exhaustive literal/regex search, and mention `zg --index` when the user wants persistent indexed semantic search. Indexing creates repository-local state, may load local models or call remote embedding providers, and should be a user-owned action.

When indexing or rebuilding, choose useful repo paths instead of indexing everything. Prefer source, tests, docs, config, and scripts. zvec-grep skips dependency, third-party, generated, build, cache, hidden, nested-git, `.gitignore`d, and over-1MB files by default; use `--include` and `--exclude` to further keep the index focused:

```bash
zg --index --embedding local/embeddinggemma-300m --include "src/**" --include "docs/**" --include "test/**" --exclude "dist/**,node_modules/**,coverage/**,.zvec-grep/**"
zg --index --rebuild --embedding local/embeddinggemma-300m --include "src/**" --exclude "dist/**,node_modules/**,coverage/**,.zvec-grep/**"
```

If `zg --status` reports incompatible settings, ask before rebuilding. Existing anonymous indexes are checked and incrementally refreshed by query commands by default; use `--no-auto-update` only when diagnosing stale-index behavior. Rebuild only when the embedding schema or index version requires it:

```bash
zg --index --rebuild --embedding local/embeddinggemma-300m
```

After an anonymous index exists, start with high-quality semantic context:

```bash
zg "<query>"
```

Prefer the default indexed/hybrid search for exploration. Agent output uses `--preview none` by default to keep results token-efficient; `--human` uses `--preview full` by default for terminal reading. Use `--limit` to control candidate width, `--preview short` for a small deterministic source window, and `--preview full` after narrowing to a few results.

Use filters early. For source-code investigations, start with implementation paths and exclude tests, fixtures, generated output, dependencies, and build artifacts unless the task is specifically about those areas. Good filters keep candidate sets smaller and reduce token use:

```bash
zg "plugin loading lifecycle" --include "src/**" --exclude "test/**,tests/**,**/*.test.*,**/*.spec.*,fixtures/**,dist/**"
zg "query planning" --include "src/**,packages/*/src/**" --exclude "test/**,tests/**,fixtures/**,node_modules/**,dist/**"
```

Indexed agent results use `--preview none` by default: essential metadata plus one representative source line when source is available. Human results use `--preview full` by default. Use `--preview short` for essential metadata plus a small deterministic source window, or `--preview full` after narrowing to a few results. `--preview` is for indexed/semantic results; with `--rg`, use ripgrep context flags such as `-A`, `-B`, or `-C` instead.

```bash
zg "plugin loading" --limit 30 --preview none
zg "plugin loading" --limit 10 --preview short
zg "plugin loading" --limit 3 --preview full
```

Use multiple quoted queries when you are exploring related ideas or comparing several names/concepts. Each query gets its own search group, and `--limit` applies per query/group:

```bash
zg "request validation" "error handling"
zg "authentication flow" "session refresh" "permission checks" --limit 5
```

Use `zg --rg` only when you need exhaustive literal/regex search, exact verification, or ripgrep-specific flags. For exploratory code understanding, prefer default `zg` indexed search first:

```bash
zg --rg "SymbolName|LogMessage"
zg --rg -F "ExactSymbolOrText" src
zg --rg -i -C 2 -g "*.ts" -g "!dist/**" "needle text" src
```

Do not call `grep` or `rg` directly for repository search. If exhaustive regex or literal matching is required, use `zg --rg`; otherwise keep using default `zg` indexed search to avoid noisy, token-heavy match lists.

Check the local anonymous index explicitly only when starting a repository investigation or diagnosing index state. Build it only when the user asks for persistent indexed semantic search:

```bash
zg --status
zg --index --embedding local/embeddinggemma-300m
zg --index --rebuild --embedding local/embeddinggemma-300m
```

For a new index, always pass `--embedding <model>` or set `ZVEC_GREP_EMBEDDING`; `zg --index` does not choose a model silently. For an existing index, rerunning `zg --index` without `--embedding` reuses the collection's stored embedding schema. Use `--embedding <model>` when intentionally choosing or changing models. Local models use `local/model`, such as `local/embeddinggemma-300m` and `local/qwen3-embedding-0.6b`. Remote models use `provider/model`, such as `qwen/text-embedding-v4`.

```bash
zg --index --embedding local/qwen3-embedding-0.6b
zg --index --embedding qwen/text-embedding-v4 --api-key "$DASHSCOPE_API_KEY"
```

Use explicit routes only when the intent is clear:

```bash
zg --fts "ExactSymbolOrToken"
zg --vector "natural language description of the code you need"
zg --fts "AuthService" "validateRequest" --vector "where incoming requests are authorized"
```

Combine default hybrid queries with `--fts` when you want broad semantic/lexical recall plus exact anchors such as symbols, flags, error codes, or log text. Put the default hybrid queries first, then add one or more `--fts` terms:

```bash
zg "authentication flow" --fts "AuthService" --include "src/**"
zg "cache invalidation" "stale data handling" --fts "CACHE_TTL" "invalidateCache" --limit 5
zg "request routing behavior" --fts "routeRequest" "RouteOptions" --include "src/**"
```

Use query filters to narrow noisy searches before reading results. Prefer adding `--include` and `--exclude` to the first query when you already know the relevant area:

```bash
zg "error handling" --include "src/**" --exclude "dist/**,node_modules/**"
zg "database migration" --include "src/**" --exclude "**/*.test.*"
zg "recent API changes" --modified-after 2026-06-01 --modified-before 2026-06-25
zg "create user" --symbol-type function --prefer-symbol
zg "service lifecycle" "resource cleanup" --symbol-type class --symbol-type function --limit 5
```

Filter notes:

- Quote glob filters so the shell does not expand them.
- `--include` and `--exclude` accept comma-separated globs and can be repeated.
- Dependency, third-party, generated, build, cache, nested-git, `.gitignore`d, and over-1MB files are skipped during indexing by default. Hidden dot paths are skipped by default; explicitly include useful hidden paths such as `.github/**`, `.codex/**`, or `.agents/**` with `--include` when needed. `.git/**` and `.zvec-grep/**` remain excluded.
- `--modified-after` and `--modified-before` accept epoch milliseconds, `YYYY-MM-DD`, or other parseable date strings.
- `--symbol-type` can be repeated; supported values are `module`, `class`, `interface`, `function`, `value`, and `alias`.
- `--prefer-symbol` is useful when a query names a symbol and you want exact indexed symbol hits ranked first.
- `--rg` is for exhaustive ripgrep search; do not combine it with indexed symbol options such as `--symbol-type` or `--prefer-symbol`. It uses rg regex syntax by default, so use `-F`/`--fixed-strings` for literal text, and use `-e`/`--regexp` when the pattern begins with `-`. It accepts common agent rg flags: `-n`, `-H`, `-F`, `-i`, `-w`, `-A`, `-B`, `-C`, `-e`/`--regexp`, `-g`/`--glob`, `--hidden`, `-t`/`--type`, `-T`/`--type-not`, `--max-depth`, `--ignore-file`, `--no-ignore`, `--smart-case`, and `--pcre2`. In `--rg`, `--limit` is a global match limit.

Combine multiple queries, explicit routes, and filters for focused context:

```bash
zg "authorization failure" "permission checks" \
  --fts "ForbiddenError" "hasPermission" \
  --include "src/**" \
  --exclude "dist/**,**/*.test.*" \
  --symbol-type function \
  --limit 5
```

Read output as:

- `path:start-end` is the returned entity.
- `outline:` is generated structure or call summary.
- `matched:` is the matching excerpt range inside the entity or `--rg` context block.
- `source:` is original file text for indexed results. `--rg` results omit the `source:` label and print numbered matching lines directly under `path:start-end`.
- `--preview none` omits `outline:`, `matched:`, and `source:` blocks but keeps metadata and one representative numbered source line when source is available.
