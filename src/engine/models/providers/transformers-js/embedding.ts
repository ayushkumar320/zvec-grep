import { join } from "node:path";
import { EngineError } from "../../../errors/index.js";
import type { Content, TextContent } from "../../../types.js";
import { defaultHome } from "../../../utils/path.js";
import {
  EmbeddingModel,
  type EmbeddingLimits,
  type EmbeddingOptions,
  type EmbeddingVector,
} from "../../embeddings.js";
import type {
  ModelProviderOptions,
  TransformersJsEmbeddingCatalogEntry,
} from "../../types.js";

type TensorLike = {
  data: ArrayLike<number>;
  dims: readonly number[];
};

type FeatureExtractionPipeline = {
  (
    texts: string[],
    options: { pooling: "mean" | "cls"; normalize: boolean },
  ): Promise<TensorLike>;
  tokenizer: {
    model_max_length: number;
  };
  dispose(): Promise<void>;
};

type TransformersJsModule = {
  pipeline(
    task: "feature-extraction",
    repo: string,
    options: {
      cache_dir: string;
      revision: string;
      dtype: "fp32" | "q8" | "q4";
    },
  ): Promise<FeatureExtractionPipeline>;
};

type TransformersJsLoader = () => Promise<TransformersJsModule>;

const DEFAULT_MODEL_CACHE_DIR = join(defaultHome(), "models");
let transformersJsImport: Promise<TransformersJsModule> | null = null;
let transformersJsLoader: TransformersJsLoader = defaultTransformersJsLoader;

async function defaultTransformersJsLoader(): Promise<TransformersJsModule> {
  try {
    return (await import("@huggingface/transformers")) as TransformersJsModule;
  } catch (cause) {
    throw new EngineError(
      "Transformers.js is required for this local embedding model",
      {
        code: "ZVEC_GREP.ENGINE.MODELS.TRANSFORMERS_JS_MISSING_DEPENDENCY",
        context: "Reinstall zvec-grep to restore @huggingface/transformers",
        cause,
      },
    );
  }
}

async function loadTransformersJs(): Promise<TransformersJsModule> {
  transformersJsImport ??= transformersJsLoader();
  return await transformersJsImport;
}

export function setTransformersJsRuntimeForTesting(
  loader: TransformersJsLoader | null,
): void {
  transformersJsImport = null;
  transformersJsLoader = loader ?? defaultTransformersJsLoader;
}

export class TransformersJsEmbeddingModel extends EmbeddingModel {
  readonly ref;
  readonly dimension;
  readonly metric;
  readonly supportedContentKinds = ["text"] as const;
  readonly limits;
  override readonly recommendedIndexConcurrency = 1;
  override readonly maxIndexConcurrency = 1;

  private readonly modelCacheDir: string;
  private pipeline: FeatureExtractionPipeline | null = null;
  private pipelineLoadPromise: Promise<FeatureExtractionPipeline> | null = null;
  private disposed = false;

  constructor(
    private readonly entry: TransformersJsEmbeddingCatalogEntry,
    options: ModelProviderOptions,
  ) {
    super();
    this.ref = { provider: entry.provider, model: entry.model } as const;
    this.dimension = entry.dimension;
    this.metric = entry.metric;
    this.limits = {
      maxBatchSize: entry.maxBatchSize,
      maxInputTokens: entry.maxInputTokens,
    } as const satisfies EmbeddingLimits;
    this.modelCacheDir =
      options.modelCacheDir ??
      process.env.ZVEC_GREP_MODEL_CACHE ??
      DEFAULT_MODEL_CACHE_DIR;
  }

  protected async doEmbed(
    contents: readonly Content[],
    options: Required<EmbeddingOptions>,
  ): Promise<EmbeddingVector[]> {
    this.ensureNotDisposed();
    const texts = (contents as readonly TextContent[]).map((content) =>
      formatText(content.text, options.purpose, this.entry),
    );

    try {
      const pipeline = await this.ensurePipeline();
      const tensor = await pipeline(texts, {
        pooling: this.entry.pooling,
        normalize: this.entry.normalize,
      });
      return tensorToVectors(tensor, texts.length, this.entry.dimension);
    } catch (cause) {
      throw new EngineError("Transformers.js embedding failed", {
        code: "ZVEC_GREP.ENGINE.MODELS.TRANSFORMERS_JS_EMBED_FAILED",
        context: `model=${this.entry.id} repo=${this.entry.repo}`,
        cause,
      });
    }
  }

  override async dispose(): Promise<void> {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    const pipeline = this.pipeline;
    this.pipeline = null;
    this.pipelineLoadPromise = null;
    await pipeline?.dispose();
  }

  private async ensurePipeline(): Promise<FeatureExtractionPipeline> {
    if (this.pipeline) {
      return this.pipeline;
    }
    if (this.pipelineLoadPromise) {
      return await this.pipelineLoadPromise;
    }

    this.pipelineLoadPromise = this.loadPipeline();
    try {
      this.pipeline = await this.pipelineLoadPromise;
      return this.pipeline;
    } finally {
      this.pipelineLoadPromise = null;
    }
  }

  private async loadPipeline(): Promise<FeatureExtractionPipeline> {
    const runtime = await loadTransformersJs();
    const pipeline = await runtime.pipeline(
      "feature-extraction",
      this.entry.repo,
      {
        cache_dir: this.modelCacheDir,
        revision: this.entry.revision,
        dtype: this.entry.dtype,
      },
    );
    pipeline.tokenizer.model_max_length = this.entry.maxInputTokens;
    return pipeline;
  }

  private ensureNotDisposed(): void {
    if (this.disposed) {
      throw new EngineError("Transformers.js embedding model is disposed", {
        code: "ZVEC_GREP.ENGINE.MODELS.TRANSFORMERS_JS_DISPOSED",
        context: `model=${this.entry.id}`,
      });
    }
  }
}

function formatText(
  text: string,
  purpose: Required<EmbeddingOptions>["purpose"],
  entry: TransformersJsEmbeddingCatalogEntry,
): string {
  const prefix = purpose === "query" ? entry.queryPrefix : entry.documentPrefix;
  return prefix ? `${prefix}${text}` : text;
}

function tensorToVectors(
  tensor: TensorLike,
  count: number,
  dimension: number,
): EmbeddingVector[] {
  if (
    tensor.dims.length !== 2 ||
    tensor.dims[0] !== count ||
    tensor.dims[1] !== dimension ||
    tensor.data.length !== count * dimension
  ) {
    throw new EngineError("Transformers.js returned an unexpected tensor", {
      code: "ZVEC_GREP.ENGINE.MODELS.TRANSFORMERS_JS_INVALID_TENSOR",
      context: `expected=${count}x${dimension} actual=${tensor.dims.join("x")}`,
    });
  }

  return Array.from({ length: count }, (_, index) =>
    Array.from(
      { length: dimension },
      (__, offset) => tensor.data[index * dimension + offset],
    ),
  );
}
