import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  DaemonInstanceLock,
  readInstanceRecord,
  serverStatus,
} from "../dist/daemon/server-controller.js";


test("daemon instance lock is exclusive, heartbeat-safe and owner-released", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-controller-"));
  t.after(async () => rm(home, { recursive: true, force: true }));
  const lock = await DaemonInstanceLock.acquire(home, "http://127.0.0.1:7999/mcp");
  t.after(async () => lock.release());
  const record = await readInstanceRecord(home);
  assert.equal(record.pid, process.pid);
  assert.equal(record.ready, false);
  await assert.rejects(
    DaemonInstanceLock.acquire(home, "http://127.0.0.1:7999/mcp"),
    /already running/i,
  );
  await lock.markReady();
  assert.equal((await readInstanceRecord(home)).ready, true);
  const status = await serverStatus(home);
  assert.equal(status.running, true);
  assert.equal(status.ready, false);
  await lock.release();
  assert.equal(await readInstanceRecord(home), undefined);
});
