import { createZvecGrep } from "../engine/service/index.js";
import type { CreateZvecGrepOptions, ZvecGrepInfoResult } from "../engine/service/types.js";
import { getEmbeddingModelCatalogEntry } from "../engine/models/index.js";
import { isEngineError } from "../engine/errors/index.js";
import type { CollectionEmbeddingSchema, IndexProgress } from "../engine/types.js";
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
import { JobScheduler, type IndexJobSnapshot, type JobSchedulerOptions } from "./job-scheduler.js";
import { EmbeddingModelPool, type EmbeddingModelPoolOptions } from "./model-pool.js";
import { IndexCoordinator } from "./index-coordinator.js";
import { inspectRoot, resolveRequestedRoot, RuntimeManager } from "./runtime-manager.js";
import type { RootRuntime } from "./root-runtime.js";
import { WatchManager, type WatchManagerOptions } from "./watch-manager.js";


const DEFAULT_LOCAL_EMBEDDING = "local/embeddinggemma-300m";

export type DaemonBackendOptions = {
  version: string;
  serviceOptions?: CreateZvecGrepOptions;
  modelPoolOptions?: EmbeddingModelPoolOptions;
  schedulerOptions?: JobSchedulerOptions;
  readCollectionIdleTtlMs?: number;
  searchWaitTimeoutMs?: number;
  runtimeIdleTtlMs?: number;
  resolveEmbeddingSchema?: (reference: string) => CollectionEmbeddingSchema;
  createService?: typeof createZvecGrep;
  watchManagerFactory?: (options: WatchManagerOptions) => WatchManager;
};

type DaemonIndexInput = ZvecGrepIndexInput & {
  changedPaths?: readonly string[];
};


export class DaemonBackend implements ZvecGrepDaemonBackend {
  readonly modelPool: EmbeddingModelPool;
  readonly runtimeManager: RuntimeManager;
  readonly scheduler: JobScheduler;
  private readonly startedAt = Date.now();
  private readonly statusCache = new Map<string, ZvecGrepInfoResult>();
  private readonly watchers = new Map<string, WatchManager>();
  private readonly indexCoordinators = new Map<string, IndexCoordinator>();
  private shuttingDown = false;
  private closePromise?: Promise<void>;


  constructor(private readonly options: DaemonBackendOptions) {
    this.modelPool = new EmbeddingModelPool({
      ...options.modelPoolOptions,
      serviceOptions: options.serviceOptions,
    });
    this.runtimeManager = new RuntimeManager({
      modelPool: this.modelPool,
      serviceOptions: options.serviceOptions,
      readCollectionIdleTtlMs: options.readCollectionIdleTtlMs,
      searchWaitTimeoutMs: options.searchWaitTimeoutMs,
      runtimeIdleTtlMs: options.runtimeIdleTtlMs,
      onRuntimeEvicted: (root) => this.closeWatcher(root),
    });
    this.scheduler = new JobScheduler(options.schedulerOptions);
  }


  async index(input: ZvecGrepIndexInput): Promise<ZvecGrepIndexResult> {
    const runtime = await this.runtimeManager.activateForIndex(input.root);
    this.ensureWatcher(runtime);
    const activeJob = this.scheduler.getByRoot(runtime.canonicalRoot);
    const followsNarrowJob = activeJob?.state === "queued" || activeJob?.state === "running"
      ? activeJob.reason === "watch"
      : false;
    const createsWork = !this.scheduler.hasActiveRoot(runtime.canonicalRoot)
      || input.rebuild === true
      || followsNarrowJob;
    const targetRevision = createsWork ? runtime.markDirty() : runtime.snapshot().dirtyRevision;
    runtime.setWriterPending(true);
    let submitted;
    try {
      submitted = this.scheduler.submit({
        canonicalRoot: runtime.canonicalRoot,
        reason: "manual",
        followupIfRunning: input.rebuild === true || followsNarrowJob,
        run: (report) => runtime.withWrite(async () => {
          await this.runIndex(runtime, input, report);
          runtime.markIndexed(targetRevision);
        }),
      });
    } catch (error) {
      runtime.setWriterPending(false);
      throw error;
    }

    if (!submitted.reused) {
      void this.scheduler.waitForRootIdle(runtime.canonicalRoot).finally(() => {
        runtime.setWriterPending(false);
      });
    }
    const job = input.wait
      ? await this.scheduler.wait(submitted.job.id)
      : submitted.job;
    if (input.wait) {
      await this.watchers.get(runtime.canonicalRoot)?.flushPending();
      await this.scheduler.waitForRootIdle(runtime.canonicalRoot);
    }
    return {
      root: runtime.canonicalRoot,
      jobId: job.id,
      state: job.state,
      reused: submitted.reused,
    };
  }


  async search(input: NormalizedSearchInput): Promise<ZvecGrepSearchResult> {
    const runtime = await this.runtimeManager.activate(input.root);
    this.ensureWatcher(runtime);
    let updateJob: IndexJobSnapshot | undefined;
    if (runtime.needsReconciliation() && input.freshness === "wait_for_fresh") {
      updateJob = await this.submitIndex(runtime, { root: runtime.canonicalRoot }, "fresh_query", true);
      if (updateJob.state !== "succeeded") {
        throw new DaemonError(
          updateJob.error?.code ?? "INDEX_FAILED",
          updateJob.error?.message ?? "Index reconciliation did not complete successfully.",
        );
      }
    }
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
    if (runtime.needsReconciliation()) {
      updateJob = await this.submitIndex(runtime, { root: runtime.canonicalRoot }, "reconcile", false);
    }
    const job = updateJob ?? this.scheduler.getByRoot(runtime.canonicalRoot);
    return {
      root: runtime.canonicalRoot,
      freshness: job && job.state !== "succeeded" ? "possibly_stale" : "fresh",
      updateJobId: job && (job.state === "queued" || job.state === "running") ? job.id : undefined,
      result,
    };
  }


  async indexStatus(input: ZvecGrepIndexStatusInput): Promise<ZvecGrepIndexStatusResult> {
    const requestedCanonicalRoot = await resolveRequestedRoot(input.root, false);
    let info: ZvecGrepInfoResult;
    try {
      info = await inspectRoot(input.root, this.options.serviceOptions);
      this.statusCache.set(requestedCanonicalRoot, info);
    } catch (error) {
      const cached = this.statusCache.get(requestedCanonicalRoot);
      if (!cached || !isEngineError(error) || error.code !== "ZVEC_GREP.ENGINE.LOCK.BUSY") {
        throw error;
      }
      info = cached;
    }
    const canonicalRoot = await resolveRequestedRoot(info.root, false);
    const runtime = this.runtimeManager.getByCanonicalRoot(canonicalRoot);
    const runtimeSnapshot = runtime?.snapshot();
    const job = this.scheduler.getByRoot(canonicalRoot);
    return {
      root: canonicalRoot,
      indexed: info.indexed,
      indexPolicy: info.indexPolicy,
      source: info.source,
      persistent: persistentStatus(info),
      runtime: runtimeSnapshot
        ? {
            watcherActive: runtimeSnapshot.watcherActive,
            dirtyRevision: runtimeSnapshot.dirtyRevision,
            indexedRevision: runtimeSnapshot.indexedRevision,
            activeJobId: job?.id,
            jobState: job?.state,
            progress: job?.progress ? formatProgress(job) : undefined,
            error: job?.error,
          }
        : undefined,
    };
  }


  async serverStatus(): Promise<ZvecGrepServerStatusResult> {
    const runtime = this.runtimeManager.snapshot();
    const models = this.modelPool.snapshot();
    const jobs = this.scheduler.snapshot();
    return {
      version: this.options.version,
      uptimeMs: Date.now() - this.startedAt,
      shuttingDown: this.shuttingDown,
      activeRuntimes: runtime.activeRuntimes,
      queuedJobs: jobs.queued,
      runningJobs: jobs.running,
      models,
    };
  }


  async close(): Promise<void> {
    if (this.closePromise) {
      return this.closePromise;
    }
    this.shuttingDown = true;
    this.closePromise = (async () => {
      await Promise.all([...this.watchers.values()].map((watcher) => watcher.close()));
      for (const root of this.watchers.keys()) {
        this.runtimeManager.getByCanonicalRoot(root)?.setWatcherActive(false);
      }
      this.watchers.clear();
      this.indexCoordinators.clear();
      await this.scheduler.close();
      await this.runtimeManager.close();
      await this.modelPool.close();
    })();
    return this.closePromise;
  }


  private async runIndex(
    runtime: RootRuntime,
    input: DaemonIndexInput,
    report: (progress: IndexProgress) => void,
  ): Promise<void> {
    const includeStatus = !input.changedPaths;
    const before = await inspectRoot(runtime.canonicalRoot, this.options.serviceOptions, includeStatus);
    if (includeStatus) this.statusCache.set(runtime.canonicalRoot, before);
    const schema = this.indexSchema(before, input);
    const lease = await this.modelPool.acquire({
      schema,
      root: runtime.canonicalRoot,
      registryHome: before.home,
    });
    let service: Awaited<ReturnType<typeof createZvecGrep>> | undefined;
    try {
      service = await (this.options.createService ?? createZvecGrep)({
        ...this.options.serviceOptions,
        root: runtime.canonicalRoot,
        embeddingModel: lease.model,
        embeddingModelOwnership: "borrowed",
        daemonInstanceToken: this.runtimeManager.instanceToken,
      });
      await service.index({
        root: runtime.canonicalRoot,
        rebuild: input.rebuild,
        changedPaths: input.changedPaths,
        onProgress: report,
      });
    } finally {
      try {
        await service?.close();
      } finally {
        lease.release();
      }
    }

    const after = await inspectRoot(runtime.canonicalRoot, this.options.serviceOptions, includeStatus);
    if (includeStatus) this.statusCache.set(runtime.canonicalRoot, after);
    if (!after.collection?.embedding) {
      throw new DaemonError("INDEX_MISSING", "Index completed without an embedding schema.");
    }
    runtime.updateModelRequest({
      schema: after.collection.embedding,
      root: runtime.canonicalRoot,
      registryHome: after.home,
    });
  }


  private async submitIndex(
    runtime: RootRuntime,
    input: DaemonIndexInput,
    reason: "manual" | "reconcile" | "fresh_query",
    wait: boolean,
  ): Promise<IndexJobSnapshot> {
    const createsWork = !this.scheduler.hasActiveRoot(runtime.canonicalRoot) || input.rebuild === true;
    const targetRevision = createsWork ? runtime.markDirty() : runtime.snapshot().dirtyRevision;
    runtime.setWriterPending(true);
    let submitted;
    try {
      submitted = this.scheduler.submit({
        canonicalRoot: runtime.canonicalRoot,
        reason,
        followupIfRunning: input.rebuild === true,
        run: (report) => runtime.withWrite(async () => {
          await this.runIndex(runtime, input, report);
          runtime.markIndexed(targetRevision);
        }),
      });
    } catch (error) {
      runtime.setWriterPending(false);
      throw error;
    }
    if (!submitted.reused) {
      void this.scheduler.waitForRootIdle(runtime.canonicalRoot).finally(() => {
        runtime.setWriterPending(false);
      });
    }
    return wait ? this.scheduler.wait(submitted.job.id) : submitted.job;
  }


  private ensureWatcher(runtime: RootRuntime): void {
    if (this.watchers.has(runtime.canonicalRoot) || this.shuttingDown) {
      return;
    }
    const coordinator = new IndexCoordinator({
      runtime,
      scheduler: this.scheduler,
      getIndexedFileCount: () => this.statusCache.get(runtime.canonicalRoot)?.status?.filesStored,
      run: async (changes, report) => {
        const changedPaths = [
          ...changes.touchedFiles,
          ...changes.rescanDirectories,
          ...changes.deletedPrefixes,
        ];
        if (!changes.forceFullReconcile && changedPaths.length === 0) {
          return;
        }
        await this.runIndex(runtime, {
          root: runtime.canonicalRoot,
          changedPaths: changes.forceFullReconcile ? undefined : changedPaths,
        }, report);
      },
    });
    const watcher = (this.options.watchManagerFactory ?? ((options) => new WatchManager(options)))({
      root: runtime.canonicalRoot,
      onChanges: (changes, reason) => { coordinator.enqueue(changes, reason); },
      onPendingChange: (pending) => runtime.setWatcherPending(pending),
    });
    watcher.start();
    this.indexCoordinators.set(runtime.canonicalRoot, coordinator);
    this.watchers.set(runtime.canonicalRoot, watcher);
    runtime.setWatcherActive(true);
  }


  private async closeWatcher(canonicalRoot: string): Promise<void> {
    const watcher = this.watchers.get(canonicalRoot);
    this.watchers.delete(canonicalRoot);
    this.indexCoordinators.delete(canonicalRoot);
    await watcher?.close();
  }


  private indexSchema(info: ZvecGrepInfoResult, input: ZvecGrepIndexInput): CollectionEmbeddingSchema {
    if (info.collection?.embedding && !input.embedding) {
      return info.collection.embedding;
    }
    const reference = input.embedding
      ?? this.options.serviceOptions?.embedding
      ?? (this.options.serviceOptions?.defaultEmbedding ? DEFAULT_LOCAL_EMBEDDING : undefined);
    if (!reference) {
      throw new DaemonError(
        "MODEL_LOAD_FAILED",
        "A new index requires embedding or an explicit server default model.",
      );
    }
    return (this.options.resolveEmbeddingSchema ?? resolveCatalogEmbeddingSchema)(reference);
  }
}


function resolveCatalogEmbeddingSchema(reference: string): CollectionEmbeddingSchema {
  const entry = getEmbeddingModelCatalogEntry(reference);
  if (!entry) {
    throw new DaemonError(
      "MODEL_LOAD_FAILED",
      `Server MVP cannot resolve embedding schema for ${reference}.`,
    );
  }
  return {
    provider: entry.provider,
    model: entry.model,
    dimension: entry.dimension,
    metric: entry.metric,
  };
}


function persistentStatus(info: ZvecGrepInfoResult): ZvecGrepIndexStatusResult["persistent"] {
  return {
    home: info.home,
    index_path: info.indexPath,
    collection: info.collection
      ? {
          id: info.collection.id,
          name: info.collection.name,
          path: info.collection.path,
          root_paths: info.collection.rootPaths.map((rootPath) => ({
            absolute_path: rootPath.absolutePath,
            recursive: rootPath.recursive,
            include: rootPath.include ? [...rootPath.include] : undefined,
            exclude: rootPath.exclude ? [...rootPath.exclude] : undefined,
          })),
          embedding: info.collection.embedding,
          index_version: info.collection.indexVersion,
          created_time: info.collection.createdTime,
          updated_time: info.collection.updatedTime,
        }
      : undefined,
    files: info.status
      ? {
          stored: info.status.filesStored,
          indexed: info.status.filesIndexed,
          pending: info.status.filesPending,
          failed: info.status.filesFailed,
          entities: info.status.entitiesIndexed,
        }
      : undefined,
    suggestion: info.suggestion,
  };
}


function formatProgress(job: IndexJobSnapshot): NonNullable<
  NonNullable<ZvecGrepIndexStatusResult["runtime"]>["progress"]
> | undefined {
  const progress = job.progress;
  if (!progress) {
    return undefined;
  }
  return {
    phase: progress.phase,
    files_total: progress.filesTotal,
    files_indexed: progress.filesIndexed,
    files_failed: progress.filesFailed,
    detail: progress.detail,
  };
}
