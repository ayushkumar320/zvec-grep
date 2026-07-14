import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";


const cliPath = resolve("dist/cli/index.js");


test("MCP exposes only search tools", async (t) => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "zvec-grep-mcp-"));
  const client = new Client({ name: "zvec-grep-test", version: "1.0.0" });
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [cliPath, "serve", "--mcp"],
    cwd: temporaryDirectory,
    env: {
      ...process.env,
      HOME: temporaryDirectory,
    },
    stderr: "pipe",
  });
  t.after(async () => {
    await client.close();
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await client.connect(transport);
  const listed = await client.listTools();
  const names = listed.tools.map((tool) => tool.name).sort();

  assert.deepEqual(names, ["zvec_grep_rg", "zvec_grep_search"]);
  assert.equal(names.includes("zvec_grep_index"), false);
  assert.equal(names.includes("zvec_grep_status"), false);
});
