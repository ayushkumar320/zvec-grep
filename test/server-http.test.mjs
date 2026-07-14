import assert from "node:assert/strict";
import { access, mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { request as httpRequest } from "node:http";
import { join } from "node:path";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { DaemonBackend } from "../dist/daemon/backend.js";
import { DaemonHttpServer } from "../dist/daemon/http-server.js";
import { EmbeddingModel } from "../dist/engine/models/embeddings.js";
import { createZvecGrep } from "../dist/index.js";


const token = "server-http-test-token-at-least-32-characters";


test("HTTP server rolls back state after a listen failure", async () => {
  const backend = {};
  const first = new DaemonHttpServer({
    host: "127.0.0.1",
    port: 0,
    token,
    version: "1.0.0",
    backend,
  });
  const firstAddress = await first.start();
  const second = new DaemonHttpServer({
    host: "127.0.0.1",
    port: firstAddress.port,
    token,
    version: "1.0.0",
    backend,
  });
  await assert.rejects(second.start(), /EADDRINUSE|address already in use/i);
  await first.close();
  const secondAddress = await second.start();
  assert.equal(secondAddress.port, firstAddress.port);
  await second.close();
});


test("Streamable HTTP serves health, MCP contracts and a real cached index search", async (t) => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "zvec-grep-http-"));
  const root = join(temporaryDirectory, "repo");
  await mkdir(join(root, "src"), { recursive: true });
  await writeFile(join(root, "src", "answer.ts"), [
    "export function answerToEverything() {",
    "  return 42;",
    "}",
    "",
  ].join("\n"));
  const canonicalRoot = await realpath(root);

  const indexModel = new TestEmbeddingModel();
  const service = await createZvecGrep({ root, embeddingModel: indexModel });
  await service.index();
  await service.close();

  let modelLoads = 0;
  const backend = new DaemonBackend({
    version: "1.0.0",
    modelPoolOptions: {
      createModel: () => {
        modelLoads += 1;
        return new TestEmbeddingModel();
      },
    },
    readCollectionIdleTtlMs: 60_000,
  });
  const server = new DaemonHttpServer({
    host: "127.0.0.1",
    port: 0,
    token,
    version: "1.0.0",
    backend,
  });
  const address = await server.start();
  const mcpUrl = new URL(`http://127.0.0.1:${address.port}/mcp`);
  t.after(async () => {
    await server.close();
    await backend.close();
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  const health = await fetch(`http://127.0.0.1:${address.port}/healthz`);
  assert.equal(health.status, 200);
  assert.deepEqual(await health.json(), { status: "ok" });

  const unauthorized = await fetch(mcpUrl, { method: "POST", body: "{}" });
  assert.equal(unauthorized.status, 401);
  const invalidHost = await rawRequestStatus(mcpUrl, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      Host: "example.com",
      "Content-Type": "application/json",
    },
    body: "{}",
  });
  assert.equal(invalidHost, 403);
  const invalidOrigin = await fetch(mcpUrl, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      Origin: "https://example.com",
      "Content-Type": "application/json",
    },
    body: "{}",
  });
  assert.equal(invalidOrigin.status, 403);
  const getMcp = await fetch(mcpUrl, {
    headers: { Authorization: `Bearer ${token}` },
  });
  assert.equal(getMcp.status, 405);

  const clients = await Promise.all([
    connectClient(mcpUrl, "client-a"),
    connectClient(mcpUrl, "client-b"),
  ]);
  t.after(async () => Promise.all(clients.map((client) => client.close())));

  const listed = await clients[0].listTools();
  assert.deepEqual(listed.tools.map((tool) => tool.name).toSorted(), [
    "zvec_grep_index",
    "zvec_grep_index_status",
    "zvec_grep_search",
    "zvec_grep_server_status",
  ]);
  const coldStatus = await clients[0].callTool({
    name: "zvec_grep_server_status",
    arguments: {},
  });
  assert.equal(coldStatus.structuredContent.active_runtimes, 0);
  assert.equal(coldStatus.structuredContent.models.loaded, 0);

  const searchRoots = [root, join(root, "src")];
  const searches = await Promise.all(clients.map((client, index) => client.callTool({
    name: "zvec_grep_search",
    arguments: { root: searchRoots[index], query: "answer to everything", limit: 3 },
  })));
  for (const search of searches) {
    assert.equal(search.isError, undefined);
    assert.equal(search.structuredContent.root, canonicalRoot);
    assert.ok(search.structuredContent.result.items.length > 0);
    assert.equal(search.structuredContent.result.items[0].file.relativePath, "src/answer.ts");
  }
  assert.equal(modelLoads, 1);

  const status = await clients[0].callTool({
    name: "zvec_grep_server_status",
    arguments: {},
  });
  assert.equal(status.structuredContent.active_runtimes, 1);
  assert.equal(status.structuredContent.models.loaded, 1);

  const unindexedRoot = join(temporaryDirectory, "unindexed");
  await mkdir(unindexedRoot);
  const missing = await clients[0].callTool({
    name: "zvec_grep_search",
    arguments: { root: unindexedRoot, query: "query" },
  });
  assert.equal(missing.isError, true);
  assert.match(missing.content[0].text, /INDEX_MISSING/);
  await assert.rejects(access(join(unindexedRoot, ".zvec-grep")));
});


async function connectClient(url, name) {
  const client = new Client({ name, version: "1.0.0" });
  const transport = new StreamableHTTPClientTransport(url, {
    requestInit: {
      headers: { Authorization: `Bearer ${token}` },
    },
  });
  await client.connect(transport);
  return client;
}


async function rawRequestStatus(url, options) {
  return await new Promise((resolve, reject) => {
    const request = httpRequest(url, {
      method: options.method,
      headers: options.headers,
    }, (response) => {
      response.resume();
      response.once("end", () => resolve(response.statusCode));
    });
    request.once("error", reject);
    request.end(options.body);
  });
}


class TestEmbeddingModel extends EmbeddingModel {
  ref = { provider: "test", model: "deterministic" };
  dimension = 8;
  metric = "cosine";
  supportedContentKinds = ["text"];
  limits = { maxBatchSize: 64 };

  async doEmbed(contents) {
    return contents.map((content) => {
      const text = content.kind === "text" ? content.text : "";
      const vector = new Array(this.dimension).fill(0);
      for (let index = 0; index < text.length; index++) {
        vector[index % vector.length] += text.charCodeAt(index) / 255;
      }
      const norm = Math.sqrt(vector.reduce((sum, value) => sum + value * value, 0)) || 1;
      return vector.map((value) => value / norm);
    });
  }
}
