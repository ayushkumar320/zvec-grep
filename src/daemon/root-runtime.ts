import type { AnonymousReadSession } from "../engine/service/zvec-grep.js";
import { openAnonymousReadSession } from "../engine/service/zvec-grep.js";
import type { ZvecGrepContextOptions, ZvecGrepContextResult } from "../engine/service/types.js";
import type {
  EmbeddingModelPool,
  ModelLease,
  ModelLeaseRequest,
} from "./model-pool.js";
import { ReadCollectionCache } from "./read-collection-cache.js";


export type RootRuntimeOptions = {
  canonicalRoot: string;
  modelPool: EmbeddingModelPool;
  modelRequest: ModelLeaseRequest;
  readCollectionIdleTtlMs?: number;
  openSession?: (lease: ModelLease) => AnonymousReadSession | Promise<AnonymousReadSession>;
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
  private modelRequest: ModelLeaseRequest;
  private closed = false;


  constructor(private readonly options: RootRuntimeOptions) {
    this.canonicalRoot = options.canonicalRoot;
    this.modelRequest = options.modelRequest;
  }


  updateModelRequest(request: ModelLeaseRequest): void {
    this.modelRequest = request;
  }


  async search(options: ZvecGrepContextOptions): Promise<ZvecGrepContextResult> {
    if (this.closed) {
      throw new Error("Root runtime is closed.");
    }
    return this.runGenerationSerial(async () => {
      if (this.closed) {
        throw new Error("Root runtime is closed.");
      }
      const request = this.modelRequest;
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


  snapshot(): { readCollectionOpen: boolean; activeReaders: number } {
    const read = this.generation?.cache.snapshot();
    return {
      readCollectionOpen: read?.open ?? false,
      activeReaders: read?.activeReaders ?? 0,
    };
  }


  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    await this.runGenerationSerial(async () => {
      const generation = this.generation;
      this.generation = undefined;
      await generation?.cache.close();
    });
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
