import assert from "node:assert/strict";
import test from "node:test";
import { IndexCoordinator } from "../dist/daemon/index-coordinator.js";
import { DaemonError } from "../dist/daemon/errors.js";
import { JobScheduler } from "../dist/daemon/job-scheduler.js";

test("changes arriving during a write are indexed by one follow-up revision", async () => {
  const scheduler = new JobScheduler({ concurrency: 1 });
  let dirtyRevision = 0;
  let indexedRevision = 0;
  let releaseFirst;
  const firstRunning = new Promise((resolve) => {
    releaseFirst = resolve;
  });
  const snapshots = [];
  const runtime = {
    canonicalRoot: "/repo",
    markDirty: () => ++dirtyRevision,
    markIndexed: (revision) => {
      indexedRevision = revision;
    },
    setWriterPending: () => {},
    withWrite: (operation) => operation(),
  };
  const coordinator = new IndexCoordinator({
    runtime,
    scheduler,
    run: async (changes) => {
      snapshots.push(changes);
      if (snapshots.length === 1) {
        await firstRunning;
      }
    },
  });
  coordinator.enqueue(change("/repo/a.ts"));
  await waitFor(() => snapshots.length === 1);
  coordinator.enqueue(change("/repo/b.ts"));
  coordinator.enqueue(change("/repo/c.ts"));
  releaseFirst();
  await scheduler.waitForRootIdle("/repo");
  assert.equal(snapshots.length, 2);
  assert.deepEqual(snapshots[0].touchedFiles, ["/repo/a.ts"]);
  assert.deepEqual(snapshots[1].touchedFiles, ["/repo/b.ts", "/repo/c.ts"]);
  assert.equal(dirtyRevision, 3);
  assert.equal(indexedRevision, 3);
  await scheduler.close();
});

test("a retry reuses the same change snapshot", async () => {
  const scheduler = new JobScheduler({
    concurrency: 1,
    maxAttempts: 2,
    retryBaseDelayMs: 1,
  });
  let dirtyRevision = 0;
  let indexedRevision = 0;
  const snapshots = [];
  const coordinator = new IndexCoordinator({
    runtime: {
      canonicalRoot: "/repo",
      markDirty: () => ++dirtyRevision,
      markIndexed: (revision) => {
        indexedRevision = revision;
      },
      setWriterPending: () => {},
      withWrite: (operation) => operation(),
    },
    scheduler,
    run: async (changes) => {
      snapshots.push(changes);
      if (snapshots.length === 1)
        throw new DaemonError("INDEX_BUSY", "busy", true);
    },
  });
  const job = coordinator.enqueue(change("/repo/a.ts"));
  assert.equal((await scheduler.wait(job.id)).state, "succeeded");
  assert.deepEqual(
    snapshots.map((snapshot) => snapshot.touchedFiles),
    [["/repo/a.ts"], ["/repo/a.ts"]],
  );
  assert.equal(indexedRevision, 1);
  await scheduler.close();
});

function change(path) {
  return {
    touchedFiles: [path],
    rescanDirectories: [],
    deletedPrefixes: [],
    forceFullReconcile: false,
  };
}

async function waitFor(predicate) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (predicate()) return;
    await new Promise((resolve) => setImmediate(resolve));
  }
  throw new Error("Condition was not reached.");
}
