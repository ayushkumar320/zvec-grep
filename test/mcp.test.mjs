import assert from "node:assert/strict";
import test from "node:test";
import { parseArgs } from "../dist/cli/args.js";
import {
  MCP_MAX_QUERY_GROUPS,
  zvecGrepCliSearchInputSchema,
  zvecGrepIndexInputSchema,
  zvecGrepSearchInputSchema,
} from "../dist/mcp/schemas.js";

test("stdio MCP entry points are not public CLI commands", () => {
  assert.throws(() => parseArgs(["serve", "--mcp"]), /removed/i);
  assert.throws(() => parseArgs(["--mcp"]), /Unknown command/i);
});

test("MCP index and search expose only supported runtime overrides", () => {
  const searchRuntime = {
    apiKey: "request-key",
    device: "auto",
  };
  const indexRuntime = {
    ...searchRuntime,
    endpoint: "https://example.test/embeddings",
  };
  assert.deepEqual(
    zvecGrepIndexInputSchema.parse({ root: "/repo", ...indexRuntime }),
    { root: "/repo", ...indexRuntime },
  );
  assert.deepEqual(
    zvecGrepSearchInputSchema.parse({ root: "/repo", ...searchRuntime }),
    {
      root: "/repo",
      ...searchRuntime,
      symbolTypes: [],
      freshness: "eventual",
      autoUpdate: true,
    },
  );
  assert.equal("endpoint" in zvecGrepSearchInputSchema.shape, false);
});

test("internal CLI search accepts the legacy combined supplemental-route bound", () => {
  const routes = Array.from(
    { length: MCP_MAX_QUERY_GROUPS * 2 },
    (_, index) => ({
      mode: index % 2 === 0 ? "fts" : "vector",
      query: `route-${index}`,
    }),
  );
  assert.equal(
    zvecGrepCliSearchInputSchema.parse({ root: "/repo", routes }).routes.length,
    MCP_MAX_QUERY_GROUPS * 2,
  );
  assert.throws(
    () =>
      zvecGrepCliSearchInputSchema.parse({
        root: "/repo",
        routes: [...routes, { mode: "fts", query: "too-many" }],
      }),
    /too_big|too big|array/i,
  );
});
