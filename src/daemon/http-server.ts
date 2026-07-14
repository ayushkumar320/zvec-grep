import { timingSafeEqual } from "node:crypto";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import type { ZvecGrepDaemonBackend } from "../mcp/tools.js";
import { handleMcpPost } from "../mcp/http-transport.js";
import { isLoopbackHost, type ServerListenAddress } from "./config.js";


const MAX_REQUEST_BYTES = 1024 * 1024;

export type DaemonHttpServerOptions = ServerListenAddress & {
  token: string;
  version: string;
  backend: ZvecGrepDaemonBackend;
  onShutdown?: () => void | Promise<void>;
};


export class DaemonHttpServer {
  private server?: Server;


  constructor(private readonly options: DaemonHttpServerOptions) {
    if (!isLoopbackHost(options.host)) {
      throw new Error("Daemon HTTP server requires a loopback host.");
    }
  }


  async start(): Promise<AddressInfo> {
    if (this.server) {
      return this.address();
    }
    this.server = createServer((request, response) => {
      void this.handleRequest(request, response).catch(() => {
        if (!response.headersSent) {
          writeJson(response, 500, {
            jsonrpc: "2.0",
            error: { code: -32603, message: "Internal server error." },
            id: null,
          });
        } else if (!response.writableEnded) {
          response.end();
        }
      });
    });
    try {
      await new Promise<void>((resolve, reject) => {
        this.server!.once("error", reject);
        this.server!.listen(this.options.port, this.options.host, () => {
          this.server!.off("error", reject);
          resolve();
        });
      });
    } catch (error) {
      this.server = undefined;
      throw error;
    }
    return this.address();
  }


  address(): AddressInfo {
    const address = this.server?.address();
    if (!address || typeof address === "string") {
      throw new Error("HTTP server is not listening.");
    }
    return address;
  }


  async close(): Promise<void> {
    const server = this.server;
    this.server = undefined;
    if (!server) {
      return;
    }
    await new Promise<void>((resolve, reject) => {
      server.close((error) => error ? reject(error) : resolve());
    });
  }


  private async handleRequest(request: IncomingMessage, response: ServerResponse): Promise<void> {
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "localhost"}`);
    if (url.pathname === "/healthz") {
      if (request.method !== "GET") {
        writeJson(response, 405, { error: "method_not_allowed" });
        return;
      }
      writeJson(response, 200, { status: "ok" });
      return;
    }

    if (url.pathname === "/control/shutdown") {
      if (!validHost(request.headers.host) || !validBearerToken(request.headers.authorization, this.options.token)) {
        writeJson(response, 401, { error: "unauthorized" });
        return;
      }
      if (request.method !== "POST") {
        writeJson(response, 405, { error: "method_not_allowed" });
        return;
      }
      writeJson(response, 202, { status: "stopping" });
      setImmediate(() => { void this.options.onShutdown?.(); });
      return;
    }

    if (url.pathname !== "/mcp") {
      writeJson(response, 404, { error: "not_found" });
      return;
    }
    if (!validHost(request.headers.host) || !validOrigin(request.headers.origin)) {
      writeJson(response, 403, { error: "forbidden_origin" });
      return;
    }
    if (!validBearerToken(request.headers.authorization, this.options.token)) {
      response.setHeader("WWW-Authenticate", "Bearer");
      writeJson(response, 401, { error: "unauthorized" });
      return;
    }
    if (request.method !== "POST") {
      writeJson(response, 405, {
        jsonrpc: "2.0",
        error: { code: -32000, message: "Method not allowed." },
        id: null,
      });
      return;
    }

    let body: unknown;
    try {
      body = await readJsonBody(request);
    } catch (error) {
      const tooLarge = error instanceof RequestBodyTooLargeError;
      writeJson(response, tooLarge ? 413 : 400, {
        jsonrpc: "2.0",
        error: { code: -32700, message: tooLarge ? "Request body too large." : "Invalid JSON." },
        id: null,
      });
      return;
    }
    await handleMcpPost(request, response, this.options.backend, this.options.version, body);
  }
}


function validHost(hostHeader: string | undefined): boolean {
  if (!hostHeader) {
    return false;
  }
  try {
    return isLoopbackHost(new URL(`http://${hostHeader}`).hostname.replace(/^\[|\]$/g, ""));
  } catch {
    return false;
  }
}


function validOrigin(originHeader: string | undefined): boolean {
  if (!originHeader) {
    return true;
  }
  try {
    const origin = new URL(originHeader);
    return origin.protocol === "http:" && isLoopbackHost(origin.hostname.replace(/^\[|\]$/g, ""));
  } catch {
    return false;
  }
}


function validBearerToken(header: string | undefined, expected: string): boolean {
  if (!header?.startsWith("Bearer ")) {
    return false;
  }
  const actual = Buffer.from(header.slice("Bearer ".length), "utf8");
  const expectedBuffer = Buffer.from(expected, "utf8");
  return actual.length === expectedBuffer.length && timingSafeEqual(actual, expectedBuffer);
}


async function readJsonBody(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let bytes = 0;
  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    bytes += buffer.length;
    if (bytes > MAX_REQUEST_BYTES) {
      throw new RequestBodyTooLargeError();
    }
    chunks.push(buffer);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}


function writeJson(response: ServerResponse, status: number, body: unknown): void {
  response.statusCode = status;
  response.setHeader("Content-Type", "application/json");
  response.setHeader("Cache-Control", "no-store");
  response.end(JSON.stringify(body));
}


class RequestBodyTooLargeError extends Error {}
