import assert from "node:assert/strict";
import test from "node:test";
import { shouldStopStdioBridge } from "../dist/mcp/stdio-bridge.js";

const connectedDaemon = {
  running: true,
  ready: true,
  pid: 1234,
  serverUrl: "http://127.0.0.1:7999/mcp",
  mcpToolset: "agent",
};

test("stdio bridge tolerates a transient health-check timeout", () => {
  assert.equal(
    shouldStopStdioBridge(connectedDaemon, {
      ...connectedDaemon,
      ready: false,
    }),
    false,
  );
});

test("stdio bridge stops when the daemon identity changes", () => {
  assert.equal(
    shouldStopStdioBridge(connectedDaemon, {
      running: false,
      ready: false,
    }),
    true,
  );
  assert.equal(
    shouldStopStdioBridge(connectedDaemon, {
      ...connectedDaemon,
      pid: 5678,
    }),
    true,
  );
  assert.equal(
    shouldStopStdioBridge(connectedDaemon, {
      ...connectedDaemon,
      serverUrl: "http://127.0.0.1:8000/mcp",
    }),
    true,
  );
});
