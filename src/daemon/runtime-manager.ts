import { access, realpath, stat } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { isAbsolute } from "node:path";
import { createZvecGrep, type ZvecGrepInfoResult } from "../engine/service/index.js";
import type { CreateZvecGrepOptions } from "../engine/service/types.js";
import { DaemonError } from "./errors.js";
import { EmbeddingModelPool } from "./model-pool.js";
import { RootRuntime } from "./root-runtime.js";


export type RuntimeManagerOptions = {
  modelPool: EmbeddingModelPool;
  serviceOptions?: CreateZvecGrepOptions;
  readCollectionIdleTtlMs?: number;
  createRuntime?: (input: {
    canonicalRoot: string;
    info: ZvecGrepInfoResult;
    modelPool: EmbeddingModelPool;
  }) => RootRuntime | Promise<RootRuntime>;
};

export type RuntimeManagerSnapshot = {
  activeRuntimes: number;
};


export class RuntimeManager {
  private readonly runtimes = new Map<string, RootRuntime>();
  private readonly creating = new Map<string, Promise<RootRuntime>>();
  private closed = false;


  constructor(private readonly options: RuntimeManagerOptions) {}


  async activate(requestedRoot: string): Promise<RootRuntime> {
    if (this.closed) {
      throw new DaemonError("DAEMON_SHUTTING_DOWN", "The daemon is shutting down.", true);
    }
    const info = await inspectRoot(requestedRoot, this.options.serviceOptions);
    if (!info.indexed || !info.collection?.embedding) {
      throw new DaemonError(
        "INDEX_MISSING",
        `No built zvec-grep index exists for ${info.root}. Call zvec_grep_index first.`,
      );
    }
    const canonicalRoot = await realpath(info.root);
    const existing = this.runtimes.get(canonicalRoot);
    if (existing) {
      existing.updateModelRequest({
        schema: info.collection.embedding,
        root: canonicalRoot,
        registryHome: info.home,
      });
      return existing;
    }

    let pending = this.creating.get(canonicalRoot);
    if (!pending) {
      pending = this.createRuntime(canonicalRoot, info);
      this.creating.set(canonicalRoot, pending);
    }
    try {
      const runtime = await pending;
      runtime.updateModelRequest({
        schema: info.collection.embedding,
        root: canonicalRoot,
        registryHome: info.home,
      });
      return runtime;
    } finally {
      this.creating.delete(canonicalRoot);
    }
  }


  snapshot(): RuntimeManagerSnapshot {
    return { activeRuntimes: this.runtimes.size };
  }


  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    await Promise.allSettled(this.creating.values());
    const runtimes = [...this.runtimes.values()];
    this.runtimes.clear();
    await Promise.all(runtimes.map((runtime) => runtime.close()));
  }


  private async createRuntime(
    canonicalRoot: string,
    info: ZvecGrepInfoResult,
  ): Promise<RootRuntime> {
    let runtime: RootRuntime;
    if (this.options.createRuntime) {
      runtime = await this.options.createRuntime({
        canonicalRoot,
        info,
        modelPool: this.options.modelPool,
      });
    } else {
      runtime = new RootRuntime({
        canonicalRoot,
        modelPool: this.options.modelPool,
        modelRequest: {
          schema: info.collection!.embedding!,
          root: canonicalRoot,
          registryHome: info.home,
        },
        readCollectionIdleTtlMs: this.options.readCollectionIdleTtlMs,
      });
    }
    if (this.closed) {
      await runtime.close();
      throw new DaemonError("DAEMON_SHUTTING_DOWN", "The daemon is shutting down.", true);
    }
    this.runtimes.set(canonicalRoot, runtime);
    return runtime;
  }
}


export async function inspectRoot(
  requestedRoot: string,
  serviceOptions: CreateZvecGrepOptions = {},
): Promise<ZvecGrepInfoResult> {
  if (!isAbsolute(requestedRoot)) {
    throw new DaemonError("ROOT_NOT_ABSOLUTE", "root must be an absolute path.");
  }
  let rootStat;
  try {
    rootStat = await stat(requestedRoot);
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "EACCES" || code === "EPERM") {
      throw new DaemonError("ROOT_PERMISSION_DENIED", "root is not readable.");
    }
    throw new DaemonError("ROOT_NOT_FOUND", "root does not exist.");
  }
  if (!rootStat.isDirectory()) {
    throw new DaemonError("ROOT_NOT_FOUND", "root is not a directory.");
  }
  try {
    await access(requestedRoot, fsConstants.R_OK);
  } catch {
    throw new DaemonError("ROOT_PERMISSION_DENIED", "root is not readable.");
  }

  const canonicalRequestedRoot = await realpath(requestedRoot);
  const service = await createZvecGrep({
    ...serviceOptions,
    root: canonicalRequestedRoot,
  });
  try {
    return await service.info({ root: canonicalRequestedRoot });
  } finally {
    await service.close();
  }
}
