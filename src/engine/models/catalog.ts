import type { EmbeddingCatalogEntry, ModelCatalog, ModelRef } from "./types.js";

export const MODEL_CATALOG = {
  openai: {
    embedding: ["text-embedding-3-small", "text-embedding-3-large"],
    ranking: [],
  },

  qwen: {
    embedding: [
      "qwen3.7-text-embedding",
      "text-embedding-v4",
      "qwen3-vl-embedding",
    ],
    ranking: ["gte-rerank-v2"],
  },

  jina: {
    embedding: ["jina-embeddings-v3"],
    ranking: ["jina-reranker-v2-base-multilingual"],
  },

  local: {
    embedding: [
      "embeddinggemma-300m",
      "qwen3-embedding-0.6b",
      "bge-small-en-v1.5",
      "all-minilm-l6-v2",
    ],
    ranking: [],
  },
} as const satisfies ModelCatalog;

export const EMBEDDING_MODEL_CATALOG = {
  "local/embeddinggemma-300m": {
    backend: "llama-cpp",
    id: "local/embeddinggemma-300m",
    provider: "local",
    model: "embeddinggemma-300m",
    uri: "hf:ggml-org/embeddinggemma-300M-GGUF/embeddinggemma-300M-Q8_0.gguf",
    dimension: 768,
    metric: "cosine",
    format: "embeddinggemma",
    contextSize: 2048,
    maxBatchSize: 16,
  },

  "local/qwen3-embedding-0.6b": {
    backend: "llama-cpp",
    id: "local/qwen3-embedding-0.6b",
    provider: "local",
    model: "qwen3-embedding-0.6b",
    uri: "hf:Qwen/Qwen3-Embedding-0.6B-GGUF/Qwen3-Embedding-0.6B-Q8_0.gguf",
    dimension: 1024,
    metric: "cosine",
    format: "qwen3",
    contextSize: 8192,
    maxBatchSize: 8,
  },

  "qwen/text-embedding-v4": {
    id: "qwen/text-embedding-v4",
    provider: "qwen",
    model: "text-embedding-v4",
    dimension: 1024,
    metric: "cosine",
  },

  "local/bge-small-en-v1.5": {
    backend: "transformers-js",
    id: "local/bge-small-en-v1.5",
    provider: "local",
    model: "bge-small-en-v1.5",
    repo: "onnx-community/bge-small-en-v1.5-ONNX",
    revision: "4a9a46c7b88fa408e650a571a1800243f26309bd",
    dtype: "q4",
    dimension: 384,
    metric: "cosine",
    pooling: "cls",
    normalize: true,
    queryPrefix: "Represent this sentence for searching relevant passages: ",
    maxInputTokens: 512,
    maxBatchSize: 4,
  },

  "local/all-minilm-l6-v2": {
    backend: "transformers-js",
    id: "local/all-minilm-l6-v2",
    provider: "local",
    model: "all-minilm-l6-v2",
    repo: "onnx-community/all-MiniLM-L6-v2-ONNX",
    revision: "aff7a1dc4e8a1ea593e6ea21e95c22ef0a25966f",
    dtype: "q4",
    dimension: 384,
    metric: "cosine",
    pooling: "mean",
    normalize: true,
    maxInputTokens: 256,
    maxBatchSize: 4,
  },
} as const satisfies Record<string, EmbeddingCatalogEntry>;

export type EmbeddingModelCatalogId = keyof typeof EMBEDDING_MODEL_CATALOG;

export function listEmbeddingModels(): EmbeddingCatalogEntry[] {
  return Object.values(EMBEDDING_MODEL_CATALOG);
}

export function getEmbeddingModelCatalogEntry(
  id: string,
): EmbeddingCatalogEntry | undefined {
  return EMBEDDING_MODEL_CATALOG[id as EmbeddingModelCatalogId];
}

export function getEmbeddingModelCatalogEntryByRef(
  ref: ModelRef,
): EmbeddingCatalogEntry | undefined {
  return listEmbeddingModels().find(
    (entry) => entry.provider === ref.provider && entry.model === ref.model,
  );
}
