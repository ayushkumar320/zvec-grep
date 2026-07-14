import assert from "node:assert/strict";
import test from "node:test";
import { routeByMode } from "../dist/client/mode-router.js";


test("auto mode chooses once before submission and never retries the selected route", async () => {
  const calls = [];
  const serverResult = await routeByMode({
    mode: "auto",
    serverAvailable: async () => true,
    server: async () => { calls.push("server"); return "server"; },
    direct: async () => { calls.push("direct"); return "direct"; },
  });
  assert.equal(serverResult, "server");
  assert.deepEqual(calls, ["server"]);

  await assert.rejects(routeByMode({
    mode: "auto",
    serverAvailable: async () => true,
    server: async () => { throw new Error("connection lost after submit"); },
    direct: async () => "direct",
  }), /connection lost/);

  assert.equal(await routeByMode({
    mode: "auto",
    serverAvailable: async () => false,
    server: async () => "server",
    direct: async () => "direct",
  }), "direct");
});
