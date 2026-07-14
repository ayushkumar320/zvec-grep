import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";
import { parseArgs } from "../dist/cli/args.js";
import { parseListenAddress } from "../dist/daemon/config.js";
import { DaemonHttpServer } from "../dist/daemon/http-server.js";


const execFileAsync = promisify(execFile);
const cliPath = resolve("dist/cli/index.js");


test("server run parses a loopback listen address", () => {
  const parsed = parseArgs(["server", "run", "--listen", "127.0.0.1:8123"]);
  assert.equal(parsed.options.server, true);
  assert.equal(parsed.options.serverAction, "run");
  assert.equal(parsed.options.listen, "127.0.0.1:8123");
  assert.deepEqual(parseListenAddress(parsed.options.listen), {
    host: "127.0.0.1",
    port: 8123,
  });
});


test("server lifecycle and client mode arguments are parsed", () => {
  for (const action of ["on", "off", "status"]) {
    const parsed = parseArgs(["server", action]);
    assert.equal(parsed.options.serverAction, action);
  }
  assert.equal(parseArgs(["--mode", "server", "query"]).options.mode, "server");
  assert.equal(parseArgs(["--mode=auto", "query"]).options.mode, "auto");
  assert.throws(() => parseArgs(["--mode", "invalid", "query"]), /direct, server, or auto/i);
  assert.throws(() => parseArgs(["--force-direct", "query"]), /requires --mode direct/i);
});


test("server run rejects non-loopback addresses and unrelated listen flags", () => {
  assert.throws(() => parseListenAddress("0.0.0.0:7999"), /loopback/i);
  assert.throws(() => new DaemonHttpServer({
    host: "0.0.0.0",
    port: 7999,
    token: "token-at-least-32-characters-long",
    version: "1.0.0",
    backend: {},
  }), /loopback/i);
  assert.throws(() => parseArgs(["--listen", "127.0.0.1:7999", "query"]), /zg server on or run/i);
});


test("server on, status and off are idempotent", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-server-cli-"));
  const port = await availablePort();
  const args = ["--home", home];
  t.after(async () => {
    await execFileAsync(process.execPath, [cliPath, "server", "off", ...args]).catch(() => undefined);
    await rm(home, { recursive: true, force: true });
  });

  const first = await execFileAsync(process.execPath, [
    cliPath, "server", "on", "--listen", `127.0.0.1:${port}`, ...args,
  ]);
  assert.match(first.stdout, /Server: ready/);
  const second = await execFileAsync(process.execPath, [cliPath, "server", "on", ...args]);
  assert.match(second.stdout, /Server: ready/);
  const status = await execFileAsync(process.execPath, [cliPath, "server", "status", ...args]);
  assert.match(status.stdout, new RegExp(`127\\.0\\.0\\.1:${port}`));
  assert.ok((await readFile(join(home, "daemon", "token"), "utf8")).trim().length >= 32);
  const stopped = await execFileAsync(process.execPath, [cliPath, "server", "off", ...args]);
  assert.match(stopped.stdout, /Server: stopped/);
  const stoppedAgain = await execFileAsync(process.execPath, [cliPath, "server", "off", ...args]);
  assert.match(stoppedAgain.stdout, /Server: stopped/);
});


async function availablePort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  return port;
}
