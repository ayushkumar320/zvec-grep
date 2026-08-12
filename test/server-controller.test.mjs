import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rm, unlink, writeFile } from "node:fs/promises";
import { hostname, tmpdir } from "node:os";
import { join } from "node:path";
import { createServer as createHttpServer } from "node:http";
import { createServer as createNetServer } from "node:net";
import test from "node:test";
import {
  DaemonInstanceLock,
  readInstanceRecord,
  serverStatus,
  startServer,
  stopServer,
} from "../dist/daemon/server-controller.js";

test("daemon instance lock is exclusive, heartbeat-safe and owner-released", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-controller-"));
  t.after(async () => rm(home, { recursive: true, force: true }));
  const port = await availablePort();
  const serverUrl = `http://127.0.0.1:${port}/mcp`;
  const lock = await DaemonInstanceLock.acquire(home, serverUrl);
  t.after(async () => lock.release());
  const record = await readInstanceRecord(home);
  assert.equal(record.pid, process.pid);
  assert.equal(record.ready, false);
  assert.equal(record.mcpToolset, "agent");
  await assert.rejects(
    DaemonInstanceLock.acquire(home, serverUrl),
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

async function availablePort() {
  const server = createNetServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  await new Promise((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
  return port;
}

test("server off waits for the recorded process after its lock disappears", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-controller-stop-"));
  const daemon = join(home, "daemon");
  await mkdir(daemon);
  const child = spawn(
    process.execPath,
    ["--input-type=module", "-e", "setInterval(() => {}, 1_000)"],
    { stdio: "ignore", windowsHide: true },
  );
  await new Promise((resolve, reject) => {
    child.once("spawn", resolve);
    child.once("error", reject);
  });
  const server = createHttpServer(async (request, response) => {
    if (request.url === "/healthz") {
      response.writeHead(200).end('{"status":"ok"}');
      return;
    }
    if (request.url === "/control/shutdown" && request.method === "POST") {
      await unlink(join(daemon, "instance.lock"));
      response.writeHead(202).end('{"status":"stopping"}');
      return;
    }
    response.writeHead(404).end();
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  t.after(async () => {
    child.kill();
    server.close();
    await rm(home, { recursive: true, force: true });
  });
  const address = server.address();
  assert.ok(address && typeof address === "object");
  await writeFile(
    join(daemon, "instance.lock"),
    `${JSON.stringify({
      pid: child.pid,
      hostname: hostname(),
      instanceToken: "slow-stopping-instance",
      startedAt: Date.now(),
      updatedAt: Date.now(),
      serverUrl: `http://127.0.0.1:${address.port}/mcp`,
      ready: true,
      mcpToolset: "agent",
    })}\n`,
  );

  await assert.rejects(
    stopServer(home, 100),
    new RegExp(`Timed out.*process ${child.pid}.*stop`, "i"),
  );
});

test("server on reports an occupied listen address without timing out", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-controller-port-"));
  const server = createNetServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  t.after(async () => {
    server.close();
    await rm(home, { recursive: true, force: true });
  });
  const address = server.address();
  assert.ok(address && typeof address === "object");

  await assert.rejects(
    startServer({
      cliPath: new URL("../dist/cli/index.js", import.meta.url).pathname,
      listen: `127.0.0.1:${address.port}`,
      home,
      timeoutMs: 200,
    }),
    /address.*already in use|already.*listening/i,
  );
});

test("a dead daemon instance record is replaced", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-controller-stale-"));
  t.after(async () => rm(home, { recursive: true, force: true }));
  const daemon = join(home, "daemon");
  await mkdir(daemon);
  await writeFile(
    join(daemon, "instance.lock"),
    `${JSON.stringify({
      pid: 2_147_483_647,
      hostname: hostname(),
      instanceToken: "stale-instance",
      startedAt: 1,
      updatedAt: 1,
      serverUrl: "http://127.0.0.1:7999/mcp",
      ready: true,
    })}\n`,
  );
  const lock = await DaemonInstanceLock.acquire(
    home,
    "http://127.0.0.1:8123/mcp",
  );
  try {
    assert.equal((await readInstanceRecord(home)).pid, process.pid);
  } finally {
    await lock.release();
  }
});

test("legacy daemon records without a toolset preserve the full MCP surface", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-controller-legacy-"));
  t.after(async () => rm(home, { recursive: true, force: true }));
  const daemon = join(home, "daemon");
  const now = Date.now();
  await mkdir(daemon);
  await writeFile(
    join(daemon, "instance.lock"),
    `${JSON.stringify({
      pid: process.pid,
      hostname: hostname(),
      instanceToken: "legacy-instance",
      startedAt: now,
      updatedAt: now,
      serverUrl: "http://127.0.0.1:7999/mcp",
      ready: false,
    })}\n`,
  );

  const record = await readInstanceRecord(home);
  assert.equal(record.mcpToolset, "full");
});
