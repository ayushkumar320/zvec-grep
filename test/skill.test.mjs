import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { ZVEC_GREP_WORKSPACE_EVIDENCE_RULES } from "../dist/prompts/zvec-grep-guidance.js";

function normalizeWhitespace(value) {
  return value.replace(/\s+/g, " ").trim();
}

test("zvec-grep skill triggers by task and selects the available transport", async () => {
  const skill = await readFile("skills/zvec-grep/SKILL.md", "utf8");
  const metadata = await readFile(
    "skills/zvec-grep/agents/openai.yaml",
    "utf8",
  );
  const fallback = await readFile(
    "skills/zvec-grep/references/cli-fallback.md",
    "utf8",
  );

  assert.match(
    skill,
    /^description: Search and index local workspaces with zvec-grep/m,
  );
  assert.match(skill, /code and non-code corpora/);
  assert.match(skill, /local, workspace, repository, or indexed material/);
  assert.match(
    skill,
    /operating inside the current project and the user asks how, where, or why its implementation or interactions work/,
  );
  assert.match(skill, /workspace-grounded semantic, fuzzy, paraphrase/);
  assert.match(skill, /or unrelated open-world/);
  assert.match(
    skill,
    /negative, incidental, or comparative workspace mentions/,
  );
  assert.match(skill, /Apply these shared workspace-evidence rules/);
  assert.match(
    skill,
    /agent is operating inside a repository or project and\s+the question asks how, where, or why its implementation, symbols, call chains/,
  );
  assert.match(
    skill,
    /For an implementation-specific question about the current checkout, do not\s+require the user to explicitly say workspace, repository, project, codebase,\s+index, or local files/,
  );
  assert.match(skill, /workspace, repository/);
  assert.match(skill, /available workspace tool alone is not evidence/);
  assert.match(
    skill,
    /semantic\s+comparison or synthesis across files, sections, or documents/,
  );
  assert.doesNotMatch(skill, /semantic or\s+cross-file retrieval/);
  assert.match(
    skill,
    /Use native `grep`\s+or `rg` for exact lexical searches unless `zvec_grep_rg` or its host-prefixed\s+equivalent is listed/,
  );
  assert.match(
    skill,
    /Use the public native HTTP MCP search tools as the primary interface when the\s+matching `zvec_grep_\*` tool is listed/,
  );
  assert.match(skill, /Call the exact listed tool directly/);
  assert.match(
    skill,
    /index lifecycle or daemon diagnostics, which are intentionally\s+kept out of the default agent MCP toolset/,
  );
  assert.match(
    skill,
    /default public MCP endpoint intentionally exposes only indexed search/,
  );
  assert.match(skill, /default Auto mode can select Server or Direct/);
  assert.doesNotMatch(skill, /explicit no-index lexical search is needed/);
  assert.match(
    skill,
    /do not probe forced Server mode and then retry forced Direct mode/,
  );
  assert.match(
    skill,
    /When an exact word, phrase, name, date,[\s\S]*?locating its occurrences is sufficient, use `zvec_grep_rg` or its\s+host-prefixed equivalent when that tool is listed; otherwise use native\s+`grep` or `rg`/,
  );
  assert.doesNotMatch(skill, /managed-rg MCP tool/);
  assert.match(
    skill,
    /Within a workspace-grounded task, call `zvec_grep_search` when wording or/,
  );
  assert.match(skill, /comparison or synthesis across files, sections/);
  assert.match(skill, /call\s+`zvec_grep_search` with the concept and anchors/);
  assert.match(skill, /make at most one focused `zvec_grep_search` probe/);
  assert.match(
    skill,
    /semantic discovery is selected because no sufficient exact anchor/,
  );
  assert.match(skill, /does not apply to\s+exact quotations/);
  assert.match(skill, /`query` for one primary hybrid query group/);
  assert.match(skill, /`queries` for one\s+or more primary hybrid groups/);
  assert.match(
    skill,
    /`fts` and `vector` add supplemental lexical\s+and semantic retrieval routes; they are not hard constraints/,
  );
  assert.doesNotMatch(skill, /ranked lexical constraints/);
  assert.match(
    skill,
    /response is one deduplicated, reranked result list with query-group\s+metadata/,
  );
  assert.match(
    skill,
    /Set `fuse: true` to collapse all primary and supplemental routes\s+into one ranked search plan/,
  );
  assert.match(skill, /"root": "\/absolute\/workspace"/);
  assert.match(skill, /"query": "how are search results ranked and fused"/);
  assert.match(skill, /"fts": \["RRF", "score"\]/);
  assert.match(skill, /"fuse": true/);
  assert.match(
    skill,
    /unrelated open-world questions,\s+current\s+external facts/,
  );
  const normalizedSkill = normalizeWhitespace(skill);
  for (const rule of ZVEC_GREP_WORKSPACE_EVIDENCE_RULES) {
    assert.ok(
      normalizedSkill.includes(normalizeWhitespace(rule)),
      `skill is missing shared workspace-evidence rule: ${rule}`,
    );
  }
  assert.match(skill, /`freshness` and `indexing`/);
  assert.match(
    skill,
    /Treat bounded source snippets in indexed results as already-read evidence/,
  );
  assert.match(
    skill,
    /open the cited\s+file only when that detail falls outside the snippet or truncation matters/,
  );
  assert.match(skill, /After authorization, use the CLI lifecycle workflow/);
  assert.doesNotMatch(skill, /zvec_grep_index(?:_drop|_status)?/);
  assert.doesNotMatch(skill, /zvec_grep_server_status/);
  assert.match(skill, /references\/cli-fallback\.md/);
  assert.doesNotMatch(skill, /Use zvec-grep through the `zg` command/);
  assert.match(
    metadata,
    /Search indexed workspaces across code and non-code content/,
  );
  assert.match(
    metadata,
    /Use \$zvec-grep to ground local requests and current-project implementation questions in the indexed workspace/,
  );
  assert.match(metadata, /even when "workspace" is not explicit/);
  assert.match(metadata, /do not use it for unrelated open-world questions/);
  assert.match(fallback, /once per workspace investigation/);
  assert.match(
    fallback,
    /Use native `grep` or `rg` for\s+no-index exact lookup/,
  );
  assert.match(fallback, /a\s+CLI fallback condition above is satisfied/);
  assert.match(fallback, /Leave `--mode` unset/);
  assert.match(fallback, /zg status\r?\n/);
  assert.doesNotMatch(fallback, /zg status --mode (?:server|direct)/);
  assert.doesNotMatch(fallback, /zg query[^\n]*--mode (?:server|direct)/);
  assert.match(fallback, /zg query "request validation"/);
  assert.match(fallback, /already attempts CPU fallback/);
  assert.match(
    fallback,
    /Do not repeat the query with an explicit CPU override/,
  );
  assert.match(
    fallback,
    /embedding context remains unavailable and exact anchors are available[\s\S]*existing indexed FTS route/,
  );
  assert.match(
    fallback,
    /Do not switch to managed ripgrep merely because semantic search is unavailable/,
  );
  assert.match(fallback, /server default model is known/);
  assert.match(fallback, /zg index\r?\n/);
  assert.doesNotMatch(fallback, /zg index[^\n]*--mode (?:server|direct)/);
});

test("benchmark skill routes exact and conceptual searches by intent", async () => {
  const skill = await readFile(
    "benchmarks/coding/skills/zvec-grep/SKILL.md",
    "utf8",
  );

  assert.match(
    skill,
    /When an exact keyword, text, symbol, filename, path, configuration key/,
  );
  assert.match(skill, /start with managed\s+ripgrep/);
  assert.match(
    skill,
    /named class, function, or symbol remains an exact anchor even when\s+its file or definition location is unknown/,
  );
  assert.match(
    skill,
    /When the exact anchor is unknown and conceptual discovery is needed/,
  );
  assert.doesNotMatch(skill, /Start exploratory searches/);
});
