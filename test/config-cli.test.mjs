import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";
import { parseArgs } from "../dist/cli/args.js";


const execFileAsync = promisify(execFile);
const cliPath = resolve("dist/cli/index.js");


test("config model set parses local runtime settings", () => {
  const parsed = parseArgs([
    "config",
    "model",
    "set",
    "local/embeddinggemma-300m",
    "--llama-gpu",
    "metal",
    "--embedding-parallelism",
    "2",
  ]);
  assert.equal(parsed.options.configAction, "model-set");
  assert.equal(parsed.options.llamaGpu, "metal");
  assert.equal(parsed.options.embeddingParallelism, 2);
  assert.deepEqual(parsed.positionals, ["local/embeddinggemma-300m"]);
});


test("config model set persists independent local model settings", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-config-cli-"));
  const workspace = join(home, "workspace");
  await mkdir(workspace);
  t.after(async () => {
    await rm(home, { recursive: true, force: true });
  });
  const env = { ...process.env, HOME: home, USERPROFILE: home };

  await execFileAsync(process.execPath, [
    cliPath,
    "config",
    "model",
    "set",
    "local/embeddinggemma-300m",
    "--llama-gpu",
    "metal",
    "--embedding-parallelism",
    "2",
  ], { env, cwd: workspace });
  await execFileAsync(process.execPath, [
    cliPath,
    "config",
    "model",
    "set",
    "local/embeddinggemma-300m",
    "--no-gpu",
  ], { env, cwd: workspace });
  await execFileAsync(process.execPath, [
    cliPath,
    "config",
    "model",
    "set",
    "local/qwen3-embedding-0.6b",
    "--no-gpu",
  ], { env, cwd: workspace });

  const config = JSON.parse(await readFile(join(home, ".zvec-grep", "config.json"), "utf8"));
  assert.deepEqual(config.models, {
    "local/embeddinggemma-300m": { llamaGpu: false, embeddingParallelism: 2 },
    "local/qwen3-embedding-0.6b": { llamaGpu: false },
  });
  await assert.rejects(access(join(workspace, ".zvec-grep")), { code: "ENOENT" });
});


test("config model set rejects missing settings and remote models", async () => {
  await assert.rejects(
    execFileAsync(process.execPath, [cliPath, "config", "model", "set", "local/embeddinggemma-300m"]),
    /requires --llama-gpu/,
  );
  await assert.rejects(
    execFileAsync(process.execPath, [cliPath, "config", "model", "set", "qwen/text-embedding-v4", "--no-gpu"]),
    /only supports local embedding models/,
  );
  await assert.rejects(
    execFileAsync(process.execPath, [cliPath, "config", "model", "set", "local/unknown", "--no-gpu"]),
    /Unsupported local embedding model/,
  );
  assert.throws(
    () => parseArgs(["config", "model", "set", "local/embeddinggemma-300m", "--no-gpu", "--gpu"]),
    /conflicts with an earlier GPU option/,
  );
  assert.throws(
    () => parseArgs(["config", "model", "set", "local/embeddinggemma-300m", "--no-gpu", "--api-key", "secret"]),
    /does not accept --api-key/,
  );
});
