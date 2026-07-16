import assert from "node:assert/strict";
import test from "node:test";
import { parseArgs } from "../dist/cli/args.js";

test("stdio MCP entry points are not public CLI commands", () => {
  assert.throws(() => parseArgs(["serve", "--mcp"]), /removed/i);
  assert.throws(() => parseArgs(["--mcp"]), /unknown option/i);
});
