import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

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
  assert.match(skill, /or for unrelated open-world/);
  assert.match(
    skill,
    /negative, incidental, or comparative workspace mentions/,
  );
  assert.match(
    skill,
    /Treat the current indexed workspace as an evidence source/,
  );
  assert.match(skill, /workspace or repository/);
  assert.match(skill, /or a workspace tool is\s+available/);
  assert.match(
    skill,
    /semantic\s+comparison or synthesis across files, sections, or documents/,
  );
  assert.doesNotMatch(skill, /semantic or\s+cross-file retrieval/);
  assert.match(
    skill,
    /Use native\s+`grep`\s+or `rg` for exact lexical searches/,
  );
  assert.match(
    skill,
    /Use the public native HTTP MCP search tools as the primary interface when the matching `zvec_grep_\*` tool is present/,
  );
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
    /When an exact word, phrase, name, date,[\s\S]*?locating its occurrences is sufficient, use the managed-rg MCP tool when\s+it is available; otherwise use native `grep` or `rg`/,
  );
  assert.match(skill, /optional full MCP toolset exposes\s+`zvec_grep_rg`/);
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
  assert.match(skill, /ranked lexical constraints within an indexed search/);
  assert.match(
    skill,
    /unrelated open-world\s+questions,\s+current external facts/,
  );
  assert.match(skill, /`freshness` and `indexing`/);
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
    /Use \$zvec-grep when the answer should be grounded in the current indexed workspace/,
  );
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
