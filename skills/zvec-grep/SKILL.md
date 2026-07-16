---
name: zvec-grep
description: Use zvec-grep instead of grep or rg: check the index state once per repository investigation when needed, use existing indexed context when available, and use explicit zg query --rg for lexical search when no index exists unless the user asks to build an index.
---

# zvec-grep

Use zvec-grep through the `zg` command for repository search instead of `grep`, `rg`, or broad manual file scans. zvec-grep provides higher-quality results by combining indexed semantic context with lexical matching, while `zg query --rg` provides an explicit no-index lexical search route.

At the start of a repository investigation, check the current repository's anonymous index status once:

```bash
zg status
```

Do not run `zg status` before every search. Reuse the initial status for subsequent `zg query` calls in the same root. Check again only when switching roots or collections, after an index/rebuild, or when a query reports an index problem that you need to diagnose.

If the index is missing, do not build it automatically and do not run a default `zg query "<query>"` search. Use `zg query --rg` for explicit exhaustive literal/regex search, and mention `zg index` when the user wants persistent indexed semantic search. Indexing creates repository-local state, may load local models or call remote embedding providers, and should be a user-owned action.

When indexing or rebuilding, choose useful repo paths instead of indexing everything. Prefer source, tests, docs, config, and scripts. zvec-grep skips dependency, third-party, generated, build, cache, hidden, nested-git, `.gitignore`d, and over-1MB files by default; use repeatable rg-style `-g`/`--glob` and `-t`/`--type` filters to keep the index focused:

```bash
zg index --embedding local/embeddinggemma-300m -g "src/**" -g "docs/**" -g "test/**" -g "!dist/**" -g "!node_modules/**"
zg index --rebuild --embedding local/embeddinggemma-300m -g "src/**" -g "!dist/**" -g "!node_modules/**" -t ts
```

If `zg status` reports incompatible settings, ask before rebuilding. Existing anonymous indexes are checked and incrementally refreshed by query commands by default; use `--no-auto-update` only when diagnosing stale-index behavior. Rebuild only when the embedding schema or index version requires it:

```bash
zg index --rebuild --embedding local/embeddinggemma-300m
```

After an anonymous index exists, start with high-quality semantic context:

```bash
zg query "<query>"
```

Prefer the default indexed/hybrid search for exploration. Agent output uses `--preview none` by default to keep results token-efficient; `--human` uses `--preview full` by default for terminal reading. Use `--limit` to control candidate width, `--preview short` for a small deterministic source window, and `--preview full` after narrowing to a few results.

Use filters early. For source-code investigations, start with implementation paths and exclude tests, fixtures, generated output, dependencies, and build artifacts unless the task is specifically about those areas. Good filters keep candidate sets smaller and reduce token use:

```bash
zg query "plugin loading lifecycle" -g "src/**" -g "!test/**" -g "!tests/**" -g "!**/*.test.*" -g "!**/*.spec.*"
zg query "query planning" -g "src/**" -g "packages/*/src/**" -g "!fixtures/**" -g "!node_modules/**" -t ts
```

Indexed agent results use `--preview none` by default: essential metadata plus one representative source line when source is available. Human results use `--preview full` by default. Use `--preview short` for essential metadata plus a small deterministic source window, or `--preview full` after narrowing to a few results. `--preview` is for indexed/semantic results; with `--rg`, use ripgrep context flags such as `-A`, `-B`, or `-C` instead.

```bash
zg query "plugin loading" --limit 30 --preview none
zg query "plugin loading" --limit 10 --preview short
zg query "plugin loading" --limit 3 --preview full
```

Use multiple quoted queries when you are exploring related ideas or comparing several names/concepts. Each query gets its own search group, and `--limit` applies per query/group:

```bash
zg query "request validation" "error handling"
zg query "authentication flow" "session refresh" "permission checks" --limit 5
```

Use `--fuse` only when those groups should contribute to one combined ranking;
then `--limit` applies to the final fused list:

```bash
zg query --hybrid "authentication flow" --fts "AuthService" --vector "credential validation" --fuse --limit 10
```

Use `zg query --rg` only when you need exhaustive literal/regex search, exact verification, or ripgrep-specific flags. For exploratory code understanding, prefer indexed `zg query` search first:

```bash
zg query --rg "SymbolName|LogMessage"
zg query --rg -F "ExactSymbolOrText" src
zg query --rg -i -C 2 -g "*.ts" -g "!dist/**" "needle text" src
```

Do not call `grep` or `rg` directly for repository search. If exhaustive regex or literal matching is required, use `zg query --rg`; otherwise keep using indexed `zg query` search to avoid noisy, token-heavy match lists.

Check the local anonymous index explicitly only when starting a repository investigation or diagnosing index state. Build it only when the user asks for persistent indexed semantic search:

```bash
zg status
zg index --embedding local/embeddinggemma-300m
zg index --rebuild --embedding local/embeddinggemma-300m
```

For a new index, always pass `--embedding <model>` or set `ZVEC_GREP_EMBEDDING`; `zg index` does not choose a model silently. For an existing index, rerunning `zg index` without `--embedding` reuses the collection's stored embedding schema. Use `--embedding <model>` when intentionally choosing or changing models. Local models use `local/model`, such as `local/embeddinggemma-300m` and `local/qwen3-embedding-0.6b`. Remote models use `provider/model`, such as `qwen/qwen3.7-text-embedding`.

```bash
zg index --embedding local/qwen3-embedding-0.6b
zg index --embedding qwen/qwen3.7-text-embedding --api-key "$DASHSCOPE_API_KEY"
```

Use explicit routes only when the intent is clear:

```bash
zg query --fts "ExactSymbolOrToken"
zg query --vector "natural language description of the code you need"
zg query --fts "AuthService" --fts "validateRequest" --vector "where incoming requests are authorized"
```

Combine default hybrid queries with `--fts` when you want broad semantic/lexical recall plus exact anchors such as symbols, flags, error codes, or log text. Put the default hybrid queries first, then add one or more `--fts` terms:

```bash
zg query "authentication flow" --fts "AuthService" -g "src/**" -t ts
zg query "cache invalidation" "stale data handling" --fts "CACHE_TTL" --fts "invalidateCache" --limit 5
zg query "request routing behavior" --fts "routeRequest" --fts "RouteOptions" -g "src/**" -t ts
```

Use query filters to narrow noisy searches before reading results. Prefer adding `-g`/`--glob`, `--iglob`, `-t`/`--type`, and `-T`/`--type-not` to the first query when you already know the relevant area:

```bash
zg query "error handling" -g "src/**" -g "!dist/**" -g "!node_modules/**"
zg query "database migration" -g "src/**" -g "!**/*.test.*"
zg query "recent API changes" --modified-after 2026-06-01 --modified-before 2026-06-25
zg query "create user" --symbol-type function --prefer-symbol
zg query "service lifecycle" "resource cleanup" --symbol-type class --symbol-type function --limit 5
```

Filter notes:

- Quote glob filters so the shell does not expand them.
- `-g`/`--glob` accepts one glob and can be repeated. Prefix a glob with `!` to exclude it. Use `--iglob` for case-insensitive globs.
- `-t`/`--type` and `-T`/`--type-not` use ripgrep's file-type names and work in indexed and rg modes.
- Type filters further restrict the glob-selected files. For example, `-g "docs/**" -t ts` selects only TypeScript files under `docs`.
- Dependency, third-party, generated, build, cache, nested-git, `.gitignore`d, and over-1MB files are skipped during indexing by default. Hidden dot paths are skipped by default; use `--hidden` when explicitly needed. `.git/**` and `.zvec-grep/**` remain excluded.
- `--modified-after` and `--modified-before` accept epoch milliseconds, `YYYY-MM-DD`, or other parseable date strings.
- `--symbol-type` can be repeated; supported values are `module`, `class`, `interface`, `function`, `value`, and `alias`.
- `--prefer-symbol` is useful when a query names a symbol and you want exact indexed symbol hits ranked first.
- `--rg` is for exhaustive ripgrep search outside `.git/**` and `.zvec-grep/**`; do not combine it with indexed symbol options such as `--symbol-type` or `--prefer-symbol`. It uses rg regex syntax by default, so use `-F` for literal text and `-e` when a pattern begins with `-`. Managed rg supports common matching, pattern-file, context, glob, type, ignore, depth, size, symlink, encoding, multiline, and regex-engine flags. Zvec-grep owns the result format, so output-changing flags such as `--json`, `--count`, `--files`, `-l`, `-o`, `--replace`, and `--vimgrep` are rejected. In `--rg`, `--limit` is a global result limit while `-m`/`--max-count` retains rg's per-file meaning.

Combine multiple queries, explicit routes, and filters for focused context:

```bash
zg query "authorization failure" "permission checks" \
  --fts "ForbiddenError" --fts "hasPermission" \
  -g "src/**" \
  -g "!dist/**" \
  -g "!**/*.test.*" \
  --symbol-type function \
  --limit 5
```

Read output as:

- `path:start-end` is the returned entity.
- `outline:` is generated structure or call summary.
- `matched:` is the matching excerpt range inside the entity or `--rg` context block.
- `source:` is original file text for indexed results. `--rg` results omit the `source:` label and print numbered matching lines directly under `path:start-end`.
- `--preview none` omits `outline:`, `matched:`, and `source:` blocks but keeps metadata and one representative numbered source line when source is available.
