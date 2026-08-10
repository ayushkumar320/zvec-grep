---
name: zvec-grep
description: Search and index local workspaces with zvec-grep across code and non-code corpora including documentation, books, research, notes, knowledge-base exports, manuals, configuration, and data. Use when the user asks to inspect, search, or ground an answer in local, workspace, repository, or indexed material; prior context makes the workspace the evidence source; the user asks whether relevant local material exists; the agent is operating inside the current project and the user asks how, where, or why its implementation or interactions work; workspace-grounded semantic, fuzzy, paraphrase, relationship, chronology, causality, comparison, or cross-file, cross-section, or cross-document synthesis is needed; local retrieval would otherwise use grep, rg, or broad file reads; or authorized index lifecycle or daemon diagnostics are requested. Do not trigger for negative, incidental, or comparative workspace mentions, tool availability alone, or unrelated open-world, current external, or web-only questions.
---

# zvec-grep

## Select the evidence source and transport

Apply these shared workspace-evidence rules:

- Treat the current workspace as an intended evidence source when the user asks
  to inspect, search, or ground the answer in local material; prior context
  established the workspace as the source; the user asks whether relevant local
  material exists; or the agent is operating inside a repository or project and
  the question asks how, where, or why its implementation, symbols, call chains,
  dependencies, lifecycle, data flow, architecture, or interactions work.
- For an implementation-specific question about the current checkout, do not
  require the user to explicitly say workspace, repository, project, codebase,
  index, or local files.
- A workspace may contain source code, documentation, books, research material,
  meeting notes, knowledge-base exports, manuals, configuration, data, or mixed
  content.
- Negative, incidental, or comparative mentions of a workspace do not establish
  workspace relevance, and an available workspace tool alone is not evidence
  that the workspace is relevant.
- Do not use workspace retrieval for unrelated open-world questions, current
  external facts, or web content that does not depend on local evidence.

Once workspace relevance is established, use indexed search for semantic
comparison or synthesis across files, sections, or documents. Use native `grep`
or `rg` for exact lexical searches unless `zvec_grep_rg` or its host-prefixed
equivalent is listed.

Use the public native HTTP MCP search tools as the primary interface when the
matching `zvec_grep_*` tool is listed. Call the exact listed tool directly; do
not run `zg`, probe the daemon through shell, or choose CLI for convenience.

Use CLI fallback only when one of these conditions is true:

- The task requires index lifecycle or daemon diagnostics, which are intentionally
  kept out of the default agent MCP toolset.
- An available MCP search tool cannot perform the required operation.
- MCP initialization, authentication, connection, or the required search call has
  failed.

Do not retry a submitted indexing write through another transport after a connection interruption.

## Use the MCP workflow

Pass the workspace's daemon-visible absolute path as `root` on every zvec-grep
workspace call.

1. When an exact word, phrase, name, date, identifier, filename, path,
   configuration key, error message, source fragment, literal, or regex is known
   and locating its occurrences is sufficient, use `zvec_grep_rg` or its
   host-prefixed equivalent when that tool is listed; otherwise use native
   `grep` or `rg`.
2. Within a workspace-grounded task, call `zvec_grep_search` when wording or
   location is unknown, or when the answer requires semantic, conceptual, fuzzy,
   or paraphrase discovery; relationships, chronology, causality, architecture,
   or data or control flow; or comparison or synthesis across files, sections,
   or documents. Search
   defaults to `freshness: "eventual"`; use
   `freshness: "wait_for_fresh"` only when the result must include all pending
   changes. Use `query` for one primary hybrid query group and `queries` for one
   or more primary hybrid groups. `fts` and `vector` add supplemental lexical
   and semantic retrieval routes; they are not hard constraints. By default,
   the response is one deduplicated, reranked result list with query-group
   metadata. Set `fuse: true` to collapse all primary and supplemental routes
   into one ranked search plan. For example:

   ```json
   {
     "root": "/absolute/workspace",
     "query": "how are search results ranked and fused",
     "fts": ["RRF", "score"],
     "fuse": true
   }
   ```
3. Within a workspace-grounded mixed task, when exact anchors are known but the
   answer requires those relationships or cross-file synthesis, call
   `zvec_grep_search` with the concept and anchors, then use `zvec_grep_rg` or
   its host-prefixed equivalent when that tool is listed, or native `grep` or
   `rg` otherwise, for focused follow-up.
4. When semantic discovery is selected because no sufficient exact anchor is
   available and the user asks whether conceptually related material exists
   locally, make at most one focused `zvec_grep_search` probe using the user's
   question plus distinctive names, dates, or terms. This probe does not apply to
   exact quotations, configuration keys, filenames, regexes, or exhaustive
   occurrence requests. Continue only when the results are relevant; otherwise
   stop local discovery and report that the indexed workspace did not establish
   the answer.
5. Read the indexed search response's `freshness` and `indexing` fields. Use
   `possibly_stale` results immediately when they are sufficient; do not call
   status merely because a background update is active.
6. Treat bounded source snippets in indexed results as already-read evidence.
   Answer from a snippet when it contains the required detail; open the cited
   file only when that detail falls outside the snippet or truncation matters.
7. Apply focused path and file-type filters early. Exclude dependencies,
   generated output, caches, build artifacts, fixtures, and logs unless the
   task concerns them.

The default public MCP endpoint intentionally exposes only indexed search. If the
index is missing, explain that indexed search requires an index and ask before
creating one. After authorization, use the CLI lifecycle workflow; never silently
create, rebuild, or drop an index. When `zvec_grep_rg` or its host-prefixed
equivalent is listed, use it for exhaustive literal or regex search, including
when the index is missing; otherwise use native `grep` or `rg`. The CLI
`zg query --rg` route remains available only under the fallback conditions
above.

## Use CLI fallback

Read [references/cli-fallback.md](references/cli-fallback.md) only after a fallback condition above is satisfied. Leave CLI mode unset for status, fallback search, and authorized indexing commands so the default Auto mode can select Server or Direct; do not probe forced Server mode and then retry forced Direct mode. Keep the selected transport consistent for the workspace task unless its availability changes.
