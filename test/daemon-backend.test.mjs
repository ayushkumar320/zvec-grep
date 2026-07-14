import assert from "node:assert/strict";
import { mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
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


test("watch changes use the path-level index pipeline and advance revisions", async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "zvec-grep-watch-backend-"));
  const root = join(temporaryDirectory, "repo");
  await mkdir(root);
  const source = join(root, "answer.ts");
  await writeFile(source, "export const answer = 42;\n");
  const service = await createZvecGrep({ root, embeddingModel: new TestEmbeddingModel() });
  await service.index();
  await service.close();
  let watcherOptions;
  let watcherCloses = 0;
  const indexedPathBatches = [];
  let markPathIndexStarted;
  let releasePathIndex;
  const pathIndexStarted = new Promise((resolve) => { markPathIndexStarted = resolve; });
  const pathIndexReleased = new Promise((resolve) => { releasePathIndex = resolve; });
  const backend = new DaemonBackend({
    version: "1.0.0",
    modelPoolOptions: { createModel: () => new TestEmbeddingModel() },
    runtimeIdleTtlMs: 1_000,
    createService: async (options) => {
      const created = await createZvecGrep(options);
      return {
        ...created,
        root: created.root,
        collections: created.collections,
        index: async (indexOptions) => {
          if (indexOptions.changedPaths) {
            indexedPathBatches.push([...indexOptions.changedPaths]);
            markPathIndexStarted();
            await pathIndexReleased;
          }
          return created.index(indexOptions);
        },
        disableIndex: (infoOptions) => created.disableIndex(infoOptions),
        info: (infoOptions) => created.info(infoOptions),
        context: (contextOptions) => created.context(contextOptions),
        close: () => created.close(),
      };
    },
    watchManagerFactory: (options) => {
      watcherOptions = options;
      return {
        start() {},
        flushPending: async () => {},
        close: async () => { watcherCloses += 1; },
      };
    },
  });
  try {
    await backend.search(searchInput(root, "answer", "wait_for_fresh"));
    await writeFile(source, "export const updatedAnswer = 43;\n");
    await watcherOptions.onChanges({
      touchedFiles: [source],
      rescanDirectories: [],
      deletedPrefixes: [],
      forceFullReconcile: false,
    });
    await pathIndexStarted;
    const manualIndex = backend.index({ root, wait: true });
    releasePathIndex();
    assert.equal((await manualIndex).state, "succeeded");
    const canonicalRoot = await realpath(root);
    await backend.scheduler.waitForRootIdle(canonicalRoot);
    const result = await backend.search({
      ...searchInput(root, "updatedAnswer", "eventual"),
      queries: undefined,
      routes: [{ mode: "fts", query: "updatedAnswer" }],
    });
    assert.match(result.result.items[0].content, /updatedAnswer/);
    assert.deepEqual(indexedPathBatches, [[source]]);
    const status = await backend.indexStatus({ root });
    assert.equal(status.runtime.watcherActive, true);
    assert.equal(status.runtime.dirtyRevision, 3);
    assert.equal(status.runtime.indexedRevision, 3);
    await waitFor(() => watcherCloses === 1);
    assert.equal((await backend.serverStatus()).activeRuntimes, 0);
  } finally {
    await backend.close();
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
});


test("daemon restart forgets runtimes and jobs but preserves index discovery", async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "zvec-grep-restart-"));
  const root = join(temporaryDirectory, "repo");
  await mkdir(root);
  await writeFile(join(root, "answer.ts"), "export const answer = 42;\n");
  const service = await createZvecGrep({ root, embeddingModel: new TestEmbeddingModel() });
  await service.index();
  await service.close();
  const options = {
    version: "1.0.0",
    modelPoolOptions: { createModel: () => new TestEmbeddingModel() },
  };
  const first = new DaemonBackend(options);
  try {
    await first.search(searchInput(root, "answer", "wait_for_fresh"));
    assert.equal((await first.serverStatus()).activeRuntimes, 1);
  } finally {
    await first.close();
  }

  const second = new DaemonBackend(options);
  try {
    const server = await second.serverStatus();
    assert.equal(server.activeRuntimes, 0);
    assert.equal(server.queuedJobs, 0);
    assert.equal(server.runningJobs, 0);
    const index = await second.indexStatus({ root });
    assert.equal(index.indexed, true);
    assert.equal(index.runtime, undefined);
  } finally {
    await second.close();
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
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


function searchInput(root, query, freshness) {
  return {
    root,
    queries: [query],
    routes: [],
    freshness,
    maxContentChars: 1_200,
  };
}


async function waitFor(predicate) {
  for (let attempt = 0; attempt < 200; attempt++) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error("Condition was not reached.");
}
