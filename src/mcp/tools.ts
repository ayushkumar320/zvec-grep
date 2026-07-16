import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ZvecGrepContextResult } from "../index.js";
import {
  normalizeSearchInput,
  type NormalizedSearchInput,
} from "./input-normalization.js";
import {
  zvecGrepIndexInputSchema,
  zvecGrepIndexOutputSchema,
  zvecGrepIndexStatusInputSchema,
  zvecGrepIndexStatusOutputSchema,
  zvecGrepSearchInputSchema,
  zvecGrepSearchOutputSchema,
  zvecGrepServerStatusInputSchema,
  zvecGrepServerStatusOutputSchema,
  type ZvecGrepIndexInput,
  type ZvecGrepIndexStatusInput,
} from "./schemas.js";
import {
  contextText,
  simplifyContextResult,
  toolResult,
} from "./result-format.js";

export type IndexJobState =
  "queued" | "running" | "succeeded" | "failed" | "cancelled";

export type ZvecGrepIndexResult = {
  root: string;
  jobId: string;
  state: IndexJobState;
  reused: boolean;
};

export type ZvecGrepSearchResult = {
  root: string;
  freshness: "fresh" | "possibly_stale";
  updateJobId?: string;
  result: ZvecGrepContextResult;
};

export type ZvecGrepIndexStatusResult = {
  root: string;
  indexed: boolean;
  indexPolicy: "enabled" | "disabled" | "undecided";
  source: "index" | "unindexed";
  persistent: {
    home: string;
    index_path: string;
    collection?: {
      id: string;
      name: string;
      path: string;
      root_paths: Array<{
        absolute_path: string;
        recursive: boolean;
        include?: string[];
        exclude?: string[];
      }>;
      embedding?: {
        provider: string;
        model: string;
        dimension: number;
        metric: string;
      } | null;
      index_version?: number | null;
      created_time: number;
      updated_time: number;
    };
    files?: {
      stored: number;
      indexed: number;
      pending: number;
      failed: number;
      entities: number;
    };
    suggestion?: string;
  };
  runtime?: {
    watcherActive: boolean;
    dirtyRevision: number;
    indexedRevision: number;
    activeJobId?: string;
    jobState?: IndexJobState;
    progress?: {
      phase: "scanning" | "indexing" | "done";
      files_total?: number;
      files_indexed?: number;
      files_failed?: number;
      detail?: string;
    };
    error?: { code: string; message: string };
  };
};

export type ZvecGrepServerStatusResult = {
  version: string;
  uptimeMs: number;
  shuttingDown: boolean;
  activeRuntimes: number;
  queuedJobs: number;
  runningJobs: number;
  models: {
    loaded: number;
    activeLeases: number;
  };
};

export interface ZvecGrepDaemonBackend {
  index(input: ZvecGrepIndexInput): Promise<ZvecGrepIndexResult>;
  search(input: NormalizedSearchInput): Promise<ZvecGrepSearchResult>;
  indexStatus(
    input: ZvecGrepIndexStatusInput,
  ): Promise<ZvecGrepIndexStatusResult>;
  serverStatus(): Promise<ZvecGrepServerStatusResult>;
}

export function createZvecGrepMcpServer(
  backend: ZvecGrepDaemonBackend,
  version: string,
): McpServer {
  const server = new McpServer(
    { name: "zvec-grep", version },
    {
      instructions: [
        "Use zvec-grep for indexed repository search.",
        "Every repository operation requires an absolute root path visible to the daemon.",
        "Call zvec_grep_index before the first zvec_grep_search. Its wait parameter defaults to false; poll zvec_grep_index_status for background progress and set wait: true only when completion is required before continuing.",
      ].join(" "),
    },
  );
  registerZvecGrepTools(server, backend);
  return server;
}

export function registerZvecGrepTools(
  server: McpServer,
  backend: ZvecGrepDaemonBackend,
): void {
  server.registerTool(
    "zvec_grep_index",
    {
      title: "Ensure zvec-grep index",
      description:
        "Activate an absolute repository root and create or incrementally update its index.",
      inputSchema: zvecGrepIndexInputSchema.shape,
      outputSchema: zvecGrepIndexOutputSchema.shape,
      annotations: {
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: true,
      },
    },
    async (input) => {
      const result = await backend.index(input);
      const structuredContent = {
        root: result.root,
        job_id: result.jobId,
        state: result.state,
        reused: result.reused,
      };
      return toolResult(
        `root: ${result.root}\njob_id: ${result.jobId}\nstate: ${result.state}\nreused: ${result.reused}`,
        structuredContent,
      );
    },
  );

  server.registerTool(
    "zvec_grep_search",
    {
      title: "Search with zvec-grep",
      description:
        "Search an existing repository index and report whether results may be stale.",
      inputSchema: zvecGrepSearchInputSchema.shape,
      outputSchema: zvecGrepSearchOutputSchema.shape,
      annotations: {
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: true,
      },
    },
    async (input) => {
      const normalized = normalizeSearchInput(input);
      const response = await backend.search(normalized);
      const structuredContent = {
        root: response.root,
        freshness: response.freshness,
        update_job_id: response.updateJobId,
        result: simplifyContextResult(
          response.result,
          normalized.maxContentChars,
        ),
      };
      const statusLines = [
        `freshness: ${response.freshness}`,
        ...(response.updateJobId
          ? [`update_job_id: ${response.updateJobId}`]
          : []),
      ];
      return toolResult(
        `${statusLines.join("\n")}\n${contextText(response.result, normalized.maxContentChars)}`,
        structuredContent,
      );
    },
  );

  server.registerTool(
    "zvec_grep_index_status",
    {
      title: "Inspect zvec-grep index status",
      description:
        "Read persisted index status and, when active, the daemon runtime and job status for an absolute root.",
      inputSchema: zvecGrepIndexStatusInputSchema.shape,
      outputSchema: zvecGrepIndexStatusOutputSchema.shape,
      annotations: {
        readOnlyHint: true,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: false,
      },
    },
    async (input) => {
      const result = await backend.indexStatus(input);
      const structuredContent = formatIndexStatus(result);
      return toolResult(
        JSON.stringify(structuredContent, null, 2),
        structuredContent,
      );
    },
  );

  server.registerTool(
    "zvec_grep_server_status",
    {
      title: "Inspect zvec-grep server status",
      description:
        "Read daemon version, queue, runtime and model-pool summary without exposing repository paths.",
      inputSchema: zvecGrepServerStatusInputSchema.shape,
      outputSchema: zvecGrepServerStatusOutputSchema.shape,
      annotations: {
        readOnlyHint: true,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: false,
      },
    },
    async () => {
      const result = await backend.serverStatus();
      const structuredContent = {
        version: result.version,
        uptime_ms: result.uptimeMs,
        shutting_down: result.shuttingDown,
        active_runtimes: result.activeRuntimes,
        queued_jobs: result.queuedJobs,
        running_jobs: result.runningJobs,
        models: {
          loaded: result.models.loaded,
          active_leases: result.models.activeLeases,
        },
      };
      return toolResult(
        JSON.stringify(structuredContent, null, 2),
        structuredContent,
      );
    },
  );
}

function formatIndexStatus(
  result: ZvecGrepIndexStatusResult,
): Record<string, unknown> {
  return {
    root: result.root,
    indexed: result.indexed,
    index_policy: result.indexPolicy,
    source: result.source,
    persistent: result.persistent,
    runtime: result.runtime
      ? {
          watcher_active: result.runtime.watcherActive,
          dirty_revision: result.runtime.dirtyRevision,
          indexed_revision: result.runtime.indexedRevision,
          active_job_id: result.runtime.activeJobId,
          job_state: result.runtime.jobState,
          progress: result.runtime.progress,
          error: result.runtime.error,
        }
      : undefined,
  };
}
