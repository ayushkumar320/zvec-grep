import type { AnonymousReadSession } from "../engine/service/zvec-grep.js";
import { openAnonymousReadSession } from "../engine/service/zvec-grep.js";
import type { ZvecGrepContextOptions, ZvecGrepContextResult } from "../engine/service/types.js";
import type {
  EmbeddingModelPool,
  ModelLease,
  ModelLeaseRequest,
} from "./model-pool.js";
import { ReadCollectionCache } from "./read-collection-cache.js";
import type { RootLease } from "./root-lease.js";
import { DaemonError } from "./errors.js";


export type RootRuntimeOptions = {
  canonicalRoot: string;
  modelPool: EmbeddingModelPool;
  modelRequest?: ModelLeaseRequest;
  rootLease?: RootLease;
  readCollectionIdleTtlMs?: number;
  openSession?: (lease: ModelLease) => AnonymousReadSession | Promise<AnonymousReadSession>;
  searchWaitTimeoutMs?: number;
  onActivity?: () => void;
};

type LeasedReadSession = AnonymousReadSession & {
  readonly modelKey: string;
};

type ReadGeneration = {
  key: string;
  cache: ReadCollectionCache<LeasedReadSession>;
};


export class RootRuntime {
  readonly canonicalRoot: string;
  private generation?: ReadGeneration;
  private generationTail: Promise<void> = Promise.resolve();
  private modelRequest?: ModelLeaseRequest;
  private dirtyRevision = 0;
  private indexedRevision = 0;
  private reconciliationRequired = true;
  private watcherActive = false;
  private watcherPending = false;
  private writerPending = false;
  private writerReady?: Promise<void>;
  private writerReadyResolve?: () => void;
  private closed = false;


  constructor(private readonly options: RootRuntimeOptions) {
    this.canonicalRoot = options.canonicalRoot;
    this.modelRequest = options.modelRequest;
  }


  updateModelRequest(request: ModelLeaseRequest): void {
    this.modelRequest = request;
  }


  async search(options: ZvecGrepContextOptions): Promise<ZvecGrepContextResult> {
    this.options.onActivity?.();
    if (this.closed) {
      throw new Error("Root runtime is closed.");
    }
    while (this.writerPending && this.writerReady) {
      await this.waitForWriter(this.writerReady);
    }
    return this.runGenerationSerial(async () => {
      if (this.closed) {
        throw new Error("Root runtime is closed.");
      }
      const request = this.modelRequest;
      if (!request) {
        throw new Error("Root runtime does not have an indexed embedding schema.");
      }
      const desiredKey = this.options.modelPool.keyFor(request);
      if (this.generation?.key !== desiredKey) {
        await this.generation?.cache.close();
        this.generation = {
          key: desiredKey,
          cache: new ReadCollectionCache({
            open: () => this.openLeasedSession(request),
            idleTtlMs: this.options.readCollectionIdleTtlMs,
            serializeOperations: true,
          }),
        };
      }

      return this.generation.cache.withRead((session) => session.context({
        ...options,
        root: this.canonicalRoot,
        autoUpdate: false,
        fallback: "disabled",
      }));
    });
  }


  setWriterPending(pending: boolean): void {
    if (pending === this.writerPending) {
      return;
    }
    this.writerPending = pending;
    if (pending) {
      this.writerReady = new Promise<void>((resolve) => {
        this.writerReadyResolve = resolve;
      });
    } else {
      this.writerReadyResolve?.();
      this.writerReadyResolve = undefined;
      this.writerReady = undefined;
    }
  }


  markDirty(): number {
    this.dirtyRevision += 1;
    return this.dirtyRevision;
  }


  markIndexed(revision = this.dirtyRevision): void {
    this.indexedRevision = Math.max(this.indexedRevision, revision);
    if (this.indexedRevision >= this.dirtyRevision) {
      this.reconciliationRequired = false;
    }
  }


  needsReconciliation(): boolean {
    return this.reconciliationRequired || this.indexedRevision < this.dirtyRevision;
  }


  setWatcherActive(active: boolean): void {
    this.watcherActive = active;
  }


  setWatcherPending(pending: boolean): void {
    this.watcherPending = pending;
    this.options.onActivity?.();
  }


  async withWrite<T>(operation: () => Promise<T>): Promise<T> {
    this.options.onActivity?.();
    try {
      return await this.runGenerationSerial(async () => {
        const generation = this.generation;
        this.generation = undefined;
        await generation?.cache.close();
        return operation();
      });
    } finally {
      this.options.onActivity?.();
    }
  }


  snapshot(): {
    readCollectionOpen: boolean;
    activeReaders: number;
    writerPending: boolean;
    dirtyRevision: number;
    indexedRevision: number;
    watcherActive: boolean;
    watcherPending: boolean;
  } {
    const read = this.generation?.cache.snapshot();
    return {
      readCollectionOpen: read?.open ?? false,
      activeReaders: read?.activeReaders ?? 0,
      writerPending: this.writerPending,
      dirtyRevision: this.dirtyRevision,
      indexedRevision: this.indexedRevision,
      watcherActive: this.watcherActive,
      watcherPending: this.watcherPending,
    };
  }


  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.watcherActive = false;
    this.watcherPending = false;
    this.setWriterPending(false);
    await this.runGenerationSerial(async () => {
      const generation = this.generation;
      this.generation = undefined;
      try {
        await generation?.cache.close();
      } finally {
        await this.options.rootLease?.release();
      }
    });
  }


  private async waitForWriter(writerReady: Promise<void>): Promise<void> {
    const timeoutMs = this.options.searchWaitTimeoutMs ?? 2_000;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        writerReady,
        new Promise<never>((_, reject) => {
          timeout = setTimeout(() => reject(new DaemonError(
            "INDEX_BUSY",
            "An index writer is pending for this root.",
            true,
          )), timeoutMs);
          timeout.unref?.();
        }),
      ]);
    } finally {
      if (timeout) {
        clearTimeout(timeout);
      }
    }
  }


  private async openLeasedSession(request: ModelLeaseRequest): Promise<LeasedReadSession> {
    const lease = await this.options.modelPool.acquire(request);
    let session: AnonymousReadSession;
    try {
      session = this.options.openSession
        ? await this.options.openSession(lease)
        : openAnonymousReadSession(this.options.canonicalRoot, lease.model);
    } catch (error) {
      lease.release();
      throw error;
    }
    let closed = false;
    return {
      root: session.root,
      modelKey: lease.key,
      context: (contextOptions) => session.context(contextOptions),
      async close() {
        if (closed) {
          return;
        }
        closed = true;
        try {
          await session.close();
        } finally {
          lease.release();
        }
      },
    };
  }


  private async runGenerationSerial<T>(operation: () => Promise<T>): Promise<T> {
    const previous = this.generationTail;
    let release!: () => void;
    this.generationTail = new Promise<void>((resolve) => {
      release = resolve;
    });
    await previous;
    try {
      return await operation();
    } finally {
      release();
    }
  }
}
