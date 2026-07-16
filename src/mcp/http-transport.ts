import type { IncomingMessage, ServerResponse } from "node:http";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import type { ZvecGrepDaemonBackend } from "./tools.js";
import { createZvecGrepMcpServer } from "./tools.js";

export async function handleMcpPost(
  request: IncomingMessage,
  response: ServerResponse,
  backend: ZvecGrepDaemonBackend,
  version: string,
  body: unknown,
): Promise<void> {
  const server = createZvecGrepMcpServer(backend, version);
  const transport = new StreamableHTTPServerTransport({
    sessionIdGenerator: undefined,
  });
  try {
    await server.connect(transport);
    await transport.handleRequest(request, response, body);
  } finally {
    await transport.close();
    await server.close();
  }
}
