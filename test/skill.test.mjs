import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("zvec-grep skill keeps native MCP tools ahead of CLI fallback", async () => {
  const skill = await readFile("skills/zvec-grep/SKILL.md", "utf8");
  const metadata = await readFile(
    "skills/zvec-grep/agents/openai.yaml",
    "utf8",
  );
  const fallback = await readFile(
    "skills/zvec-grep/references/cli-fallback.md",
    "utf8",
  );

  assert.match(skill, /^description: Use zvec-grep before grep or rg/m);
  assert.match(skill, /Use zvec-grep before raw `grep` or `rg`/);
  assert.match(skill, /Use native HTTP MCP tools as the primary interface/);
  assert.match(skill, /`wait` parameter defaults to false/i);
  assert.match(skill, /Poll `zvec_grep_index_status` only when completion/);
  assert.match(skill, /server default is known; never guess a model/);
  assert.match(skill, /zvec_grep_index_status/);
  assert.match(skill, /Call `zvec_grep_search` first/);
  assert.match(skill, /`freshness` and `indexing`/);
  assert.doesNotMatch(skill, /Call `zvec_grep_index_status` once at the start/);
  assert.match(skill, /references\/cli-fallback\.md/);
  assert.doesNotMatch(skill, /Use zvec-grep through the `zg` command/);
  assert.match(
    metadata,
    /MCP-first indexed repository search with CLI fallback/,
  );
  assert.match(
    metadata,
    /Use \$zvec-grep before grep or rg for repository search/,
  );
  assert.match(fallback, /zg status --mode server/);
  assert.match(fallback, /zg query "request validation"/);
  assert.match(fallback, /server default model is known/);
  assert.match(fallback, /zg index --mode server/);
});
