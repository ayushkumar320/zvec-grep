import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  createZvecGrep,
  type CreateZvecGrepOptions,
  type ZvecGrep,
} from "../index.js";
import {
  contextOptionsFromRgInput,
  contextOptionsFromSearchInput,
} from "../mcp/input-normalization.js";
import { contextToolResult } from "../mcp/result-format.js";
import {
  legacyRgInputSchema,
  legacySearchInputSchema,
} from "../mcp/schemas.js";
import { readPackageVersion } from "./version.js";


export async function runMcpServer(options: CreateZvecGrepOptions): Promise<void> {
  const zvecGrep = await createZvecGrep(options);
  const server = createMcpServer(zvecGrep);
  const transport = new StdioServerTransport();

  const close = async () => {
    await zvecGrep.close();
    await server.close();
  };
  process.once("SIGINT", () => {
    void close().finally(() => process.exit(130));
  });
  process.once("SIGTERM", () => {
    void close().finally(() => process.exit(143));
  });

  await server.connect(transport);
}


function createMcpServer(zvecGrep: ZvecGrep): McpServer {
  const server = new McpServer(
    {
      name: "zvec-grep",
      version: readPackageVersion(),
    },
    {
      instructions: [
        "Use zvec-grep for repository code search before grep, rg, or broad file reads.",
        "Use zvec_grep_search for indexed semantic and lexical retrieval, and zvec_grep_rg for explicit no-index lexical search.",
        "Index management and status inspection are CLI-only operations.",
      ].join(" "),
    },
  );

  server.registerTool(
    "zvec_grep_search",
    {
      title: "zvec-grep indexed search",
      description: "Search an indexed repository with zvec-grep hybrid semantic and lexical retrieval. Like the CLI, this may refresh a stale anonymous index unless autoUpdate is false.",
      inputSchema: legacySearchInputSchema.shape,
      annotations: {
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: true,
      },
    },
    async (input) => {
      const result = await zvecGrep.context(contextOptionsFromSearchInput(input));
      return contextToolResult(result, input.maxContentChars);
    },
  );

  server.registerTool(
    "zvec_grep_rg",
    {
      title: "zvec-grep no-index lexical search",
      description: "Run explicit no-index lexical search through zvec-grep managed ripgrep output.",
      inputSchema: legacyRgInputSchema.shape,
      annotations: {
        readOnlyHint: true,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: false,
      },
    },
    async (input) => {
      const result = await zvecGrep.context(contextOptionsFromRgInput(input));
      return contextToolResult(result, input.maxContentChars);
    },
  );

  return server;
}
