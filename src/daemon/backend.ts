import type { CreateZvecGrepOptions } from "../engine/service/types.js";
import type { NormalizedSearchInput } from "../mcp/input-normalization.js";
import type {
  ZvecGrepDaemonBackend,
  ZvecGrepIndexResult,
  ZvecGrepIndexStatusResult,
  ZvecGrepSearchResult,
  ZvecGrepServerStatusResult,
} from "../mcp/tools.js";
import type { ZvecGrepIndexInput, ZvecGrepIndexStatusInput } from "../mcp/schemas.js";
import { DaemonError } from "./errors.js";
import { EmbeddingModelPool, type EmbeddingModelPoolOptions } from "./model-pool.js";
import { RuntimeManager } from "./runtime-manager.js";


export type DaemonBackendOptions = {
  version: string;
  serviceOptions?: CreateZvecGrepOptions;
  modelPoolOptions?: EmbeddingModelPoolOptions;
  readCollectionIdleTtlMs?: number;
};


export class DaemonBackend implements ZvecGrepDaemonBackend {
  readonly modelPool: EmbeddingModelPool;
  readonly runtimeManager: RuntimeManager;
  private readonly startedAt = Date.now();
  private shuttingDown = false;


  constructor(private readonly options: DaemonBackendOptions) {
    this.modelPool = new EmbeddingModelPool({
      ...options.modelPoolOptions,
      serviceOptions: options.serviceOptions,
    });
    this.runtimeManager = new RuntimeManager({
      modelPool: this.modelPool,
      serviceOptions: options.serviceOptions,
      readCollectionIdleTtlMs: options.readCollectionIdleTtlMs,
    });
  }


  async index(_input: ZvecGrepIndexInput): Promise<ZvecGrepIndexResult> {
    throw new DaemonError(
      "INDEX_NOT_AVAILABLE",
      "Background indexing is not available in this server phase.",
    );
  }


  async search(input: NormalizedSearchInput): Promise<ZvecGrepSearchResult> {
    const runtime = await this.runtimeManager.activate(input.root);
    const result = await runtime.search({
      queries: input.queries,
      routes: input.routes,
      limit: input.limit,
      trace: input.trace,
      preferSymbol: input.preferSymbol,
      symbolTypes: input.symbolTypes,
      includePaths: input.includePaths,
      excludePaths: input.excludePaths,
      modifiedAfter: input.modifiedAfter,
      modifiedBefore: input.modifiedBefore,
      autoUpdate: false,
      fallback: "disabled",
    });
    return {
      root: runtime.canonicalRoot,
      freshness: "possibly_stale",
      result,
    };
  }


  async indexStatus(input: ZvecGrepIndexStatusInput): Promise<ZvecGrepIndexStatusResult> {
    void input;
    throw new DaemonError(
      "INDEX_STATUS_NOT_AVAILABLE",
      "Index status is not available in this server phase.",
    );
  }


  async serverStatus(): Promise<ZvecGrepServerStatusResult> {
    const runtime = this.runtimeManager.snapshot();
    const models = this.modelPool.snapshot();
    return {
      version: this.options.version,
      uptimeMs: Date.now() - this.startedAt,
      shuttingDown: this.shuttingDown,
      activeRuntimes: runtime.activeRuntimes,
      queuedJobs: 0,
      runningJobs: 0,
      models,
    };
  }


  async close(): Promise<void> {
    if (this.shuttingDown) {
      return;
    }
    this.shuttingDown = true;
    await this.runtimeManager.close();
    await this.modelPool.close();
  }
}
