import {
  Client,
  StreamableHTTPClientTransport,
  type ClientContext,
  type JSONRPCRequest,
  type Notification,
  type Progress,
  type Result,
} from "@modelcontextprotocol/client";
import { ResultSchema } from "@modelcontextprotocol/core";
import { Server } from "@modelcontextprotocol/server";
import type { ServerContext } from "@modelcontextprotocol/server";
import { StdioServerTransport } from "@modelcontextprotocol/server/stdio";
import { resolveClientToken } from "../daemon/config.js";
import { serverStatus, startServer } from "../daemon/server-controller.js";
import type { McpToolset } from "./toolset.js";
import { LONG_RUNNING_MCP_TIMEOUT_MS } from "./progress-heartbeat.js";

export async function runStdioBootstrapBridge(options: {
  cliPath: string;
  version: string;
  home?: string;
  tokenFile?: string;
  listen?: string;
  mcpToolset?: McpToolset;
}): Promise<void> {
  const status = await startServer({
    cliPath: options.cliPath,
    home: options.home,
    tokenFile: options.tokenFile,
    listen: options.listen,
    mcpToolset: options.mcpToolset,
  });
  if (!status.serverUrl) {
    throw new Error("zvec-grep server became ready without an MCP URL");
  }

  const downstreamHolder: { server?: Server } = {};
  let closed = false;
  let resolveClosed!: () => void;
  const closedPromise = new Promise<void>((resolve) => {
    resolveClosed = resolve;
  });
  const finish = (): void => {
    if (closed) return;
    closed = true;
    resolveClosed();
  };

  const upstream = new Client(
    { name: "zvec-grep-stdio-bridge", version: options.version },
    {
      capabilities: {
        elicitation: { form: {}, url: {} },
        roots: { listChanged: true },
        sampling: {},
      },
      versionNegotiation: { mode: { pin: "2026-07-28" } },
    },
  );
  upstream.fallbackRequestHandler = async (
    request: JSONRPCRequest,
    context: ClientContext,
  ) => {
    if (!downstreamHolder.server) {
      throw new Error("stdio MCP client is not connected");
    }
    return forwardRequest(
      downstreamHolder.server,
      request,
      context.mcpReq.signal,
      context.mcpReq._meta,
    );
  };
  upstream.fallbackNotificationHandler = async (notification: Notification) => {
    await downstreamHolder.server?.notification(notification);
  };
  upstream.onerror = (error) => console.error(error.message);
  upstream.onclose = finish;

  const token = await resolveClientToken({
    home: options.home,
    tokenFile: options.tokenFile,
  });
  const upstreamTransport = new StreamableHTTPClientTransport(
    new URL(status.serverUrl),
    {
      requestInit: {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
      },
    },
  );
  await upstream.connect(upstreamTransport);

  const downstream = new Server(
    upstream.getServerVersion() ?? {
      name: "zvec-grep",
      version: options.version,
    },
    {
      capabilities: upstream.getServerCapabilities() ?? {},
      instructions: upstream.getInstructions(),
    },
  );
  downstream.fallbackRequestHandler = async (
    request: JSONRPCRequest,
    context: ServerContext,
  ) =>
    forwardRequest(
      upstream,
      request,
      context.mcpReq.signal,
      context.mcpReq._meta,
      context.mcpReq.notify,
    );
  downstream.fallbackNotificationHandler = async (
    notification: Notification,
  ) => {
    await upstream.notification(notification);
  };
  downstreamHolder.server = downstream;
  downstream.onerror = (error) => console.error(error.message);
  downstream.onclose = finish;

  const downstreamTransport = new StdioServerTransport();
  let monitorRunning = false;
  let daemonFailure: Error | undefined;
  const monitor = setInterval(() => {
    if (closed || monitorRunning) return;
    monitorRunning = true;
    void serverStatus(options.home)
      .then(async (current) => {
        if (
          current.ready &&
          current.pid === status.pid &&
          current.serverUrl === status.serverUrl
        ) {
          return;
        }
        daemonFailure = new Error(
          "zvec-grep daemon stopped while the stdio bridge was connected",
        );
        finish();
        await downstream.close().catch(() => undefined);
      })
      .finally(() => {
        monitorRunning = false;
      });
  }, 2_000);
  monitor.unref?.();
  try {
    await downstream.connect(downstreamTransport);
    await closedPromise;
  } finally {
    clearInterval(monitor);
    await Promise.allSettled([upstream.close(), downstream.close()]);
  }
  if (daemonFailure) throw daemonFailure;
}

async function forwardRequest(
  peer: Client | Server,
  request: JSONRPCRequest,
  signal: AbortSignal,
  meta?: Record<string, unknown>,
  notify?: (notification: Notification) => Promise<void>,
): Promise<Result> {
  const progressToken = meta?.progressToken;
  const forwardedRequest: JSONRPCRequest = meta
    ? {
        ...request,
        params: {
          ...(request.params ?? {}),
          _meta: meta,
        },
      }
    : request;
  return peer.request(forwardedRequest, ResultSchema, {
    signal,
    timeout: LONG_RUNNING_MCP_TIMEOUT_MS,
    resetTimeoutOnProgress: true,
    onprogress:
      progressToken === undefined || !notify
        ? undefined
        : (progress: Progress) =>
            notify({
              method: "notifications/progress",
              params: { ...progress, progressToken },
            }),
  });
}
