import { readFile } from "node:fs/promises";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { daemonTokenPath } from "../daemon/config.js";


export class DaemonClient {
  constructor(private readonly options: { serverUrl: string; home?: string }) {}


  async callTool(name: string, args: Record<string, unknown>): Promise<Record<string, unknown>> {
    const token = (await readFile(daemonTokenPath(this.options.home), "utf8")).trim();
    const client = new Client({ name: "zvec-grep-cli", version: "1.0.0" });
    const transport = new StreamableHTTPClientTransport(new URL(this.options.serverUrl), {
      requestInit: { headers: { Authorization: `Bearer ${token}` } },
    });
    try {
      await client.connect(transport);
      const result = await client.callTool({ name, arguments: args });
      if (result.isError) {
        const text = Array.isArray(result.content)
          ? result.content.find((item) => item.type === "text")?.text
          : undefined;
        throw new Error(text ?? `${name} failed`);
      }
      return (result.structuredContent ?? {}) as Record<string, unknown>;
    } finally {
      await client.close().catch(() => undefined);
    }
  }
}
