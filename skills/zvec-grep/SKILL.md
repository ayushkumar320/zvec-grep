---
name: zvec-grep
description: Use native zvec_grep_* MCP tools for repository code search, index status, indexing, and daemon diagnostics whenever those tools are available. Never replace an available MCP tool with the zg shell command; use the CLI only after MCP initialization or the required tool actually fails, or for explicit no-index lexical search that MCP does not provide.
---

# zvec-grep

## Select the transport

Use native HTTP MCP tools as the primary interface. If the matching `zvec_grep_*` tool is present, call it directly; do not run `zg`, probe the daemon through shell, or choose CLI for convenience.

Use CLI fallback only when one of these conditions is true:

- MCP initialization, authentication, or connection has actually failed.
- The required MCP tool is absent from the current task.
- The repository has no index, the user has not authorized indexing, and an explicit no-index lexical search is needed.

Do not infer MCP failure merely because shell execution is available. Do not retry a submitted indexing write through another transport after a connection interruption.

## Use the MCP workflow

Pass the repository's daemon-visible absolute path as `root` on every repository call.

1. Call `zvec_grep_index_status` once at the start of a repository investigation. Reuse that result unless the root changes, an index operation completes, or an index error requires another check.
2. When an index exists, call `zvec_grep_search` for exploration and exact retrieval. Search defaults to `freshness: "eventual"`; use `freshness: "wait_for_fresh"` only when the result must include all pending changes. Use hybrid `queries` for concepts, `fts` for exact lexical anchors, and `vector` for semantic-only intent.
3. Apply focused `include` and `exclude` filters early. Exclude dependencies, generated output, caches, build artifacts, fixtures, and logs unless the task concerns them.
4. Call `zvec_grep_index` only when the user requests persistent indexing. Never silently create or rebuild an index. Its `wait` parameter defaults to false: submit the job in the background and poll `zvec_grep_index_status`; set `wait: true` only when completion is required before continuing.
5. Call `zvec_grep_server_status` only for daemon diagnostics, not before ordinary searches.

If the index is missing, explain that indexed search requires an index. Ask before creating one. For exhaustive literal or regex search without an index, use the CLI fallback documented in [references/cli-fallback.md](references/cli-fallback.md).

Use multiple queries when comparing related concepts. Narrow broad results before requesting larger source context. Treat returned file paths, ranges, outlines, matched excerpts, and source as the evidence for subsequent targeted reads.

## Use CLI fallback

Read [references/cli-fallback.md](references/cli-fallback.md) only after a fallback condition above is satisfied. Keep the selected transport consistent for the investigation unless its availability changes.
