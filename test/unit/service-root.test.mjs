import assert from "node:assert/strict";
import { mkdir, mkdtemp, realpath, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { workspaceIndexLocation } from "../../dist/engine/service/root.js";

test("workspace index locations resolve an existing index symlink", async () => {
  const temporaryRoot = await mkdtemp(join(tmpdir(), "zvec-grep-root-"));

  try {
    const corpusRoot = join(temporaryRoot, "corpus");
    const indexHome = join(corpusRoot, ".zvec-grep");
    const workspaceRoot = join(temporaryRoot, "workspace");
    await mkdir(indexHome, { recursive: true });
    await mkdir(workspaceRoot, { recursive: true });
    await symlink(indexHome, join(workspaceRoot, ".zvec-grep"));

    const canonicalHome = await realpath(indexHome);
    const canonicalRoot = await realpath(corpusRoot);
    assert.deepEqual(workspaceIndexLocation(workspaceRoot), {
      root: canonicalRoot,
      home: canonicalHome,
      manifestPath: join(canonicalHome, "manifest.json"),
      indexPath: join(canonicalHome, "index.zvec"),
    });
  } finally {
    await rm(temporaryRoot, { recursive: true, force: true });
  }
});
