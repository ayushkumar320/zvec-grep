import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { hostname, tmpdir } from "node:os";
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


test("a dead daemon instance record is replaced", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-controller-stale-"));
  t.after(async () => rm(home, { recursive: true, force: true }));
  const daemon = join(home, "daemon");
  await mkdir(daemon);
  await writeFile(join(daemon, "instance.lock"), `${JSON.stringify({
    pid: 2_147_483_647,
    hostname: hostname(),
    instanceToken: "stale-instance",
    startedAt: 1,
    updatedAt: 1,
    serverUrl: "http://127.0.0.1:7999/mcp",
    ready: true,
  })}\n`);
  const lock = await DaemonInstanceLock.acquire(home, "http://127.0.0.1:8123/mcp");
  try {
    assert.equal((await readInstanceRecord(home)).pid, process.pid);
  } finally {
    await lock.release();
  }
});
