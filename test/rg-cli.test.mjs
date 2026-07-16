import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";

const execFileAsync = promisify(execFile);
const cliPath = resolve("dist/cli/index.js");

test("managed rg runs locally when indexed operations use server mode", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "zvec-grep-rg-server-mode-"));
  await mkdir(join(root, "src"));
  await writeFile(
    join(root, "src", "answer.ts"),
    "export const exactNeedle = 42;\n",
  );
  t.after(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const result = await execFileAsync(
    process.execPath,
    [cliPath, "--rg", "-n", "exactNeedle", "src"],
    {
      cwd: root,
      env: { ...process.env, ZVEC_GREP_MODE: "server" },
    },
  );

  assert.match(result.stdout, /src\/answer\.ts:1/);
  assert.match(result.stdout, /exactNeedle/);
});
