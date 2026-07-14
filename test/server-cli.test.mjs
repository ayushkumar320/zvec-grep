import assert from "node:assert/strict";
import test from "node:test";
import { parseArgs } from "../dist/cli/args.js";
import { parseListenAddress } from "../dist/daemon/config.js";
import { DaemonHttpServer } from "../dist/daemon/http-server.js";


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


test("server run rejects non-loopback addresses and unrelated listen flags", () => {
  assert.throws(() => parseListenAddress("0.0.0.0:7999"), /loopback/i);
  assert.throws(() => new DaemonHttpServer({
    host: "0.0.0.0",
    port: 7999,
    token: "token-at-least-32-characters-long",
    version: "1.0.0",
    backend: {},
  }), /loopback/i);
  assert.throws(() => parseArgs(["--listen", "127.0.0.1:7999", "query"]), /zg server run/i);
});
