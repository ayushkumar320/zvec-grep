import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { DaemonBackend } from "../dist/daemon/backend.js";
import { EmbeddingModel } from "../dist/engine/models/embeddings.js";
import { createZvecGrep } from "../dist/index.js";


test("index releases its model lease when service creation fails", async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "zvec-grep-backend-"));
  const root = join(temporaryDirectory, "repo");
  await mkdir(root);
  const backend = new DaemonBackend({
    version: "1.0.0",
    modelPoolOptions: {
      createModel: () => ({ dispose: async () => {} }),
    },
    resolveEmbeddingSchema: () => ({
      provider: "test",
      model: "deterministic",
      dimension: 8,
      metric: "cosine",
    }),
    createService: async () => {
      throw new Error("service creation failed");
    },
  });
  try {
    const result = await backend.index({
      root,
      embedding: "test/deterministic",
      wait: true,
    });
    assert.equal(result.state, "failed");
    assert.equal(backend.modelPool.snapshot().activeLeases, 0);
  } finally {
    await backend.close();
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
});


test("wait_for_fresh reports a failed reconciliation instead of returning stale results", async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "zvec-grep-freshness-"));
  const root = join(temporaryDirectory, "repo");
  await mkdir(root);
  await writeFile(join(root, "answer.ts"), "export const answer = 42;\n");
  const service = await createZvecGrep({ root, embeddingModel: new TestEmbeddingModel() });
  await service.index();
  await service.close();
  const backend = new DaemonBackend({
    version: "1.0.0",
    modelPoolOptions: { createModel: () => new TestEmbeddingModel() },
    createService: async () => {
      throw new Error("reconciliation failed");
    },
  });
  try {
    await assert.rejects(backend.search({
      root,
      queries: ["answer"],
      routes: [],
      freshness: "wait_for_fresh",
      maxContentChars: 1_200,
    }), /reconciliation failed/);
  } finally {
    await backend.close();
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
});


test("concurrent backend close calls wait for the same shutdown drain", async () => {
  const backend = new DaemonBackend({ version: "1.0.0" });
  let release;
  backend.scheduler.submit({
    canonicalRoot: "/repo",
    reason: "manual",
    run: () => new Promise((resolve) => { release = resolve; }),
  });
  while (!release) {
    await new Promise((resolve) => setImmediate(resolve));
  }
  let secondClosed = false;
  const first = backend.close();
  const second = backend.close().then(() => { secondClosed = true; });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(secondClosed, false);
  release();
  await Promise.all([first, second]);
  assert.equal(secondClosed, true);
});


class TestEmbeddingModel extends EmbeddingModel {
  ref = { provider: "test", model: "deterministic" };
  dimension = 8;
  metric = "cosine";
  supportedContentKinds = ["text"];
  limits = { maxBatchSize: 64 };

  async doEmbed(contents) {
    return contents.map(() => [1, 0, 0, 0, 0, 0, 0, 0]);
  }
}
