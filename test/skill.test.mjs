import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

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
