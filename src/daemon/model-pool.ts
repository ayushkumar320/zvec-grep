import type { CreateZvecGrepOptions } from "../engine/service/types.js";
import {
  createEmbeddingModelForSchema,
  embeddingModelPoolKeyForSchema,
} from "../engine/service/zvec-grep.js";
import type { EmbeddingModel } from "../engine/models/embeddings.js";
import type { CollectionEmbeddingSchema } from "../engine/types.js";


export type ModelLeaseRequest = {
  schema: CollectionEmbeddingSchema;
  root: string;
  registryHome: string;
};

export type ModelLease = {
  readonly model: EmbeddingModel;
  readonly key: string;
  release(): void;
};

export type ModelPoolSnapshot = {
  loaded: number;
  activeLeases: number;
};

export type EmbeddingModelPoolOptions = {
  idleTtlMs?: number;
  maxLoadedModels?: number;
  serviceOptions?: CreateZvecGrepOptions;
  createModel?: (request: ModelLeaseRequest) => EmbeddingModel | Promise<EmbeddingModel>;
  keyForRequest?: (request: ModelLeaseRequest) => string;
};

type ModelEntry = {
  key: string;
  model?: EmbeddingModel;
  loading?: Promise<EmbeddingModel>;
  leases: number;
  lastUsedAt: number;
  idleTimer?: ReturnType<typeof setTimeout>;
  retired: boolean;
};


export class EmbeddingModelPool {
  private readonly entries = new Map<string, ModelEntry>();
  private readonly idleTtlMs: number;
  private readonly maxLoadedModels: number;
  private readonly createModel: (request: ModelLeaseRequest) => EmbeddingModel | Promise<EmbeddingModel>;
  private readonly keyForRequest: (request: ModelLeaseRequest) => string;
  private closed = false;


  constructor(options: EmbeddingModelPoolOptions = {}) {
    this.idleTtlMs = options.idleTtlMs ?? 15 * 60_000;
    this.maxLoadedModels = options.maxLoadedModels ?? 1;
    this.createModel = options.createModel ?? ((request) => createEmbeddingModelForSchema(
      request.schema,
      request.root,
      request.registryHome,
      options.serviceOptions,
    ));
    this.keyForRequest = options.keyForRequest ?? ((request) => embeddingModelPoolKeyForSchema(
      request.schema,
      request.root,
      request.registryHome,
      options.serviceOptions,
    ));
  }


  async acquire(request: ModelLeaseRequest): Promise<ModelLease> {
    if (this.closed) {
      throw new Error("Embedding model pool is closed.");
    }

    const key = this.keyFor(request);
    let entry = this.entries.get(key);
    if (!entry) {
      entry = {
        key,
        leases: 0,
        lastUsedAt: Date.now(),
        retired: false,
      };
      this.entries.set(key, entry);
    }

    if (entry.idleTimer) {
      clearTimeout(entry.idleTimer);
      entry.idleTimer = undefined;
    }

    if (!entry.model) {
      entry.loading ??= Promise.resolve(this.createModel(request));
      try {
        entry.model = await entry.loading;
      } catch (error) {
        this.entries.delete(key);
        throw error;
      } finally {
        entry.loading = undefined;
      }
    }

    entry.leases += 1;
    entry.lastUsedAt = Date.now();
    await this.trimIdleEntries(key);
    let released = false;
    return {
      model: entry.model,
      key,
      release: () => {
        if (released) {
          return;
        }
        released = true;
        this.release(entry!);
      },
    };
  }


  keyFor(request: ModelLeaseRequest): string {
    return this.keyForRequest(request);
  }


  snapshot(): ModelPoolSnapshot {
    let activeLeases = 0;
    let loaded = 0;
    for (const entry of this.entries.values()) {
      activeLeases += entry.leases;
      if (entry.model) {
        loaded += 1;
      }
    }
    return { loaded, activeLeases };
  }


  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    const disposals: Promise<void>[] = [];
    for (const entry of this.entries.values()) {
      if (entry.idleTimer) {
        clearTimeout(entry.idleTimer);
      }
      entry.retired = true;
      if (entry.leases === 0 && entry.model) {
        disposals.push(entry.model.dispose());
      }
    }
    await Promise.all(disposals);
    for (const [key, entry] of this.entries) {
      if (entry.leases === 0) {
        this.entries.delete(key);
      }
    }
  }


  private release(entry: ModelEntry): void {
    entry.leases = Math.max(0, entry.leases - 1);
    entry.lastUsedAt = Date.now();
    if (entry.leases > 0) {
      return;
    }

    if (entry.retired || this.closed || this.idleTtlMs === 0) {
      void this.disposeEntry(entry);
      return;
    }

    const usedAt = entry.lastUsedAt;
    entry.idleTimer = setTimeout(() => {
      if (entry.leases === 0 && entry.lastUsedAt === usedAt) {
        void this.disposeEntry(entry);
      }
    }, this.idleTtlMs);
    entry.idleTimer.unref?.();
  }


  private async trimIdleEntries(exceptKey: string): Promise<void> {
    const loaded = [...this.entries.values()].filter((entry) => entry.model);
    if (loaded.length <= this.maxLoadedModels) {
      return;
    }

    const candidates = loaded
      .filter((entry) => entry.key !== exceptKey && entry.leases === 0)
      .sort((left, right) => left.lastUsedAt - right.lastUsedAt);
    while (this.snapshot().loaded > this.maxLoadedModels && candidates.length > 0) {
      await this.disposeEntry(candidates.shift()!);
    }
  }


  private async disposeEntry(entry: ModelEntry): Promise<void> {
    if (entry.leases > 0 || !entry.model) {
      return;
    }
    if (entry.idleTimer) {
      clearTimeout(entry.idleTimer);
      entry.idleTimer = undefined;
    }
    const model = entry.model;
    entry.model = undefined;
    this.entries.delete(entry.key);
    await model.dispose();
  }
}
