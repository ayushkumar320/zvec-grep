import assert from "node:assert/strict";
import test from "node:test";
import { EmbeddingModelPool } from "../dist/daemon/model-pool.js";
import { ReadCollectionCache } from "../dist/daemon/read-collection-cache.js";
import { RootRuntime } from "../dist/daemon/root-runtime.js";
import { createZvecGrep } from "../dist/index.js";

test("read collection cache opens once, serializes operations and waits for readers before close", async () => {
  let opens = 0;
  let closes = 0;
  let activeOperations = 0;
  let maxActiveOperations = 0;
  let releaseFirst;
  const firstBlocked = new Promise((resolve) => {
    releaseFirst = resolve;
  });
  let markFirstStarted;
  const firstStarted = new Promise((resolve) => {
    markFirstStarted = resolve;
  });
  const cache = new ReadCollectionCache({
    open: async () => {
      opens += 1;
      return {
        close: async () => {
          closes += 1;
        },
      };
    },
    idleTtlMs: 60_000,
  });

  const first = cache.withRead(async () => {
    activeOperations += 1;
    maxActiveOperations = Math.max(maxActiveOperations, activeOperations);
    markFirstStarted();
    await firstBlocked;
    activeOperations -= 1;
    return "first";
  });
  await firstStarted;
  const second = cache.withRead(async () => {
    activeOperations += 1;
    maxActiveOperations = Math.max(maxActiveOperations, activeOperations);
    activeOperations -= 1;
    return "second";
  });
  const close = cache.close();
  await Promise.resolve();
  assert.equal(closes, 0);

  releaseFirst();
  assert.deepEqual(await Promise.all([first, second]), ["first", "second"]);
  await close;
  assert.equal(opens, 1);
  assert.equal(closes, 1);
  assert.equal(maxActiveOperations, 1);
});

test("embedding model pool single-flights loads and disposes after the final lease", async () => {
  let creates = 0;
  let disposals = 0;
  const model = {
    dispose: async () => {
      disposals += 1;
    },
  };
  const pool = new EmbeddingModelPool({
    idleTtlMs: 0,
    createModel: async () => {
      creates += 1;
      await Promise.resolve();
      return model;
    },
  });
  const request = {
    schema: {
      provider: "local",
      model: "test",
      dimension: 3,
      metric: "cosine",
    },
    root: "/tmp/repo",
    registryHome: "/tmp/repo/.zvec-grep",
  };

  const [first, second] = await Promise.all([
    pool.acquire(request),
    pool.acquire(request),
  ]);
  assert.equal(creates, 1);
  assert.deepEqual(pool.snapshot(), { loaded: 1, activeLeases: 2 });
  first.release();
  assert.equal(disposals, 0);
  second.release();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(disposals, 1);
  assert.deepEqual(pool.snapshot(), { loaded: 0, activeLeases: 0 });
  await pool.close();
});

test("model pool rolls back an unreturned lease when capacity trimming fails", async () => {
  const pool = new EmbeddingModelPool({
    idleTtlMs: 60_000,
    maxLoadedModels: 1,
    keyForRequest: (request) => request.schema.model,
    createModel: (request) => ({
      dispose: async () => {
        if (request.schema.model === "model-a") {
          throw new Error("dispose failed");
        }
      },
    }),
  });
  const first = await pool.acquire(modelRequest("model-a"));
  first.release();
  await assert.rejects(pool.acquire(modelRequest("model-b")), /dispose failed/);
  assert.equal(pool.snapshot().activeLeases, 0);
  await pool.close();
});

test("model pool close drains an in-flight load without returning a lease", async () => {
  let finishLoad;
  let disposals = 0;
  const pool = new EmbeddingModelPool({
    createModel: () =>
      new Promise((resolve) => {
        finishLoad = () =>
          resolve({
            dispose: async () => {
              disposals += 1;
            },
          });
      }),
  });
  const acquiring = pool.acquire(modelRequest("model-a"));
  while (!finishLoad) {
    await new Promise((resolve) => setImmediate(resolve));
  }
  const closing = pool.close();
  finishLoad();
  await assert.rejects(acquiring, /pool is closed/);
  await closing;
  assert.equal(disposals, 1);
  assert.deepEqual(pool.snapshot(), { loaded: 0, activeLeases: 0 });
});

test("service does not dispose a borrowed embedding model", async () => {
  let disposals = 0;
  const model = {
    dispose: async () => {
      disposals += 1;
    },
  };
  const service = await createZvecGrep({
    root: process.cwd(),
    embeddingModel: model,
    embeddingModelOwnership: "borrowed",
  });
  await service.close();
  assert.equal(disposals, 0);
});

test("root runtime releases model leases when the read collection closes", async () => {
  let sessionCloses = 0;
  let modelDisposals = 0;
  const pool = new EmbeddingModelPool({
    idleTtlMs: 0,
    keyForRequest: (request) => request.schema.model,
    createModel: () => ({
      dispose: async () => {
        modelDisposals += 1;
      },
    }),
  });
  const runtime = new RootRuntime({
    canonicalRoot: "/tmp/repo",
    modelPool: pool,
    modelRequest: modelRequest("model-a"),
    readCollectionIdleTtlMs: 0,
    openSession: async () => ({
      root: "/tmp/repo",
      context: async () => emptyContextResult(),
      close: async () => {
        sessionCloses += 1;
      },
    }),
  });

  await runtime.search({ query: "query" });
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(sessionCloses, 1);
  assert.equal(modelDisposals, 1);
  assert.deepEqual(pool.snapshot(), { loaded: 0, activeLeases: 0 });
  await runtime.close();
  await pool.close();
});

test("root runtime replaces a cached session when the embedding schema changes", async () => {
  let modelLoads = 0;
  let sessionCloses = 0;
  const pool = new EmbeddingModelPool({
    idleTtlMs: 0,
    maxLoadedModels: 2,
    keyForRequest: (request) => request.schema.model,
    createModel: () => {
      modelLoads += 1;
      return { dispose: async () => {} };
    },
  });
  const runtime = new RootRuntime({
    canonicalRoot: "/tmp/repo",
    modelPool: pool,
    modelRequest: modelRequest("model-a"),
    readCollectionIdleTtlMs: 60_000,
    openSession: async () => ({
      root: "/tmp/repo",
      context: async () => emptyContextResult(),
      close: async () => {
        sessionCloses += 1;
      },
    }),
  });

  await runtime.search({ query: "first" });
  runtime.updateModelRequest(modelRequest("model-b"));
  await runtime.search({ query: "second" });
  assert.equal(modelLoads, 2);
  assert.equal(sessionCloses, 1);
  await runtime.close();
  await pool.close();
});

test("root runtime releases its daemon lease when read cache close fails", async () => {
  let releases = 0;
  const pool = new EmbeddingModelPool({
    createModel: () => ({ dispose: async () => {} }),
  });
  const runtime = new RootRuntime({
    canonicalRoot: "/tmp/repo",
    modelPool: pool,
    modelRequest: modelRequest("model-a"),
    rootLease: {
      root: "/tmp/repo",
      release: async () => {
        releases += 1;
      },
    },
    readCollectionIdleTtlMs: 60_000,
    openSession: async () => ({
      root: "/tmp/repo",
      context: async () => emptyContextResult(),
      close: async () => {
        throw new Error("session close failed");
      },
    }),
  });
  await runtime.search({ query: "query" });
  await assert.rejects(runtime.close(), /session close failed/);
  assert.equal(releases, 1);
  await pool.close();
});

test("root runtime initial probe marks a clean index reconciled", async () => {
  const pool = new EmbeddingModelPool({
    createModel: () => ({ dispose: async () => {} }),
  });
  const runtime = new RootRuntime({
    canonicalRoot: "/tmp/repo",
    modelPool: pool,
    modelRequest: modelRequest("model-a"),
  });

  assert.equal(runtime.needsReconciliation(), true);
  assert.equal(await runtime.probeInitialFreshness(async () => true), "fresh");
  assert.equal(runtime.needsReconciliation(), false);
  await runtime.close();
  await pool.close();
});

test("root runtime initial probe does not hide pending watcher changes", async () => {
  const pool = new EmbeddingModelPool({
    createModel: () => ({ dispose: async () => {} }),
  });
  const runtime = new RootRuntime({
    canonicalRoot: "/tmp/repo",
    modelPool: pool,
    modelRequest: modelRequest("model-a"),
  });
  let finishProbe;
  const probe = runtime.probeInitialFreshness(
    () =>
      new Promise((resolve) => {
        finishProbe = resolve;
      }),
  );
  runtime.setWatcherPending(true);
  finishProbe(true);

  assert.equal(await probe, "stale");
  assert.equal(runtime.needsReconciliation(), true);
  await runtime.close();
  await pool.close();
});

function modelRequest(model) {
  return {
    schema: { provider: "test", model, dimension: 3, metric: "cosine" },
    root: "/tmp/repo",
    registryHome: "/tmp/repo/.zvec-grep",
  };
}

function emptyContextResult() {
  return {
    query: "query",
    root: "/tmp/repo",
    source: "index",
    coverage: "ranked_sample",
    diagnostics: {},
    items: [],
  };
}
