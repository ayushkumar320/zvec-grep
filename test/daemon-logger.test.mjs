import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  createDaemonLogger,
  opaqueIdentity,
  rootIdentity,
} from "../dist/daemon/logger.js";

test("structured daemon logs omit credentials and full query text", async (t) => {
  const home = await mkdtemp(join(tmpdir(), "zvec-grep-logger-"));
  t.after(async () => rm(home, { recursive: true, force: true }));
  const logger = createDaemonLogger(home);
  logger.event("request.completed", {
    request_id: "request-1",
    root_id: rootIdentity("/private/repository"),
    model_id: opaqueIdentity("provider/model/endpoint"),
    duration_ms: 12,
    token: "secret-token-value",
    api_key: "secret-api-key-value",
    query_text: "complete sensitive query text",
  });
  await logger.flush();

  const path = join(home, "daemon", "logs", "server.log");
  const text = await readFile(path, "utf8");
  const record = JSON.parse(text.trim());
  assert.equal(record.event, "request.completed");
  assert.equal(record.request_id, "request-1");
  assert.equal(record.duration_ms, 12);
  assert.doesNotMatch(
    text,
    /secret-token|secret-api-key|sensitive query|private\/repository/,
  );
  if (process.platform !== "win32")
    assert.equal((await stat(path)).mode & 0o777, 0o600);
});
