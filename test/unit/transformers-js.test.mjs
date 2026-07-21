import assert from "node:assert/strict";
import test from "node:test";
import {
  TransformersJsEmbeddingModel,
  setTransformersJsRuntimeForTesting,
} from "../../dist/engine/models/providers/transformers-js/embedding.js";

function entry(overrides = {}) {
  return {
    backend: "transformers-js",
    id: "local/test-transformer",
    provider: "local",
    model: "test-transformer",
    repo: "test/model-ONNX",
    revision: "0123456789abcdef",
    dtype: "q8",
    dimension: 3,
    metric: "cosine",
    pooling: "cls",
    normalize: true,
    queryPrefix: "query: ",
    documentPrefix: "passage: ",
    maxInputTokens: 512,
    maxBatchSize: 32,
    ...overrides,
  };
}

test("Transformers.js adapter fixes artifact recipe and formats query/document inputs", async (t) => {
  const loads = [];
  const calls = [];
  let disposals = 0;
  const extractor = Object.assign(
    async (texts, options) => {
      calls.push({ texts, options });
      return {
        dims: [texts.length, 3],
        data: Float32Array.from(
          texts.flatMap((_, index) => [index + 0.1, index + 0.2, index + 0.3]),
        ),
      };
    },
    {
      tokenizer: { model_max_length: 4096 },
      async dispose() {
        disposals++;
      },
    },
  );
  setTransformersJsRuntimeForTesting(async () => ({
    async pipeline(task, repo, options) {
      loads.push({ task, repo, options });
      return extractor;
    },
  }));
  t.after(() => setTransformersJsRuntimeForTesting(null));

  const model = new TransformersJsEmbeddingModel(entry(), {
    apiKey: "",
    modelCacheDir: "/tmp/model-cache",
  });
  assert.deepEqual(
    await model.embed(
      [
        { kind: "text", text: "find auth" },
        { kind: "text", text: "find parser" },
      ],
      { purpose: "query" },
    ),
    [
      Array.from(Float32Array.from([0.1, 0.2, 0.3])),
      Array.from(Float32Array.from([1.1, 1.2, 1.3])),
    ],
  );
  await model.embed([{ kind: "text", text: "implementation" }]);

  assert.deepEqual(loads, [
    {
      task: "feature-extraction",
      repo: "test/model-ONNX",
      options: {
        cache_dir: "/tmp/model-cache",
        revision: "0123456789abcdef",
        dtype: "q8",
      },
    },
  ]);
  assert.equal(extractor.tokenizer.model_max_length, 512);
  assert.deepEqual(calls, [
    {
      texts: ["query: find auth", "query: find parser"],
      options: { pooling: "cls", normalize: true },
    },
    {
      texts: ["passage: implementation"],
      options: { pooling: "cls", normalize: true },
    },
  ]);

  await model.dispose();
  await model.dispose();
  assert.equal(disposals, 1);
  await assert.rejects(
    model.embed([{ kind: "text", text: "after dispose" }]),
    /disposed/,
  );
});

test("Transformers.js adapter validates the returned batch tensor", async (t) => {
  const extractor = Object.assign(
    async () => ({ dims: [1, 2], data: new Float32Array(2) }),
    { tokenizer: { model_max_length: 4096 }, async dispose() {} },
  );
  setTransformersJsRuntimeForTesting(async () => ({
    async pipeline() {
      return extractor;
    },
  }));
  t.after(() => setTransformersJsRuntimeForTesting(null));

  const model = new TransformersJsEmbeddingModel(entry(), { apiKey: "" });
  await assert.rejects(
    model.embed([{ kind: "text", text: "value" }]),
    (error) =>
      error.message === "Transformers.js embedding failed" &&
      error.cause?.message === "Transformers.js returned an unexpected tensor",
  );
  await model.dispose();
});

test("Transformers.js adapter maps Metal to WebGPU", async (t) => {
  const loads = [];
  const extractor = Object.assign(
    async () => ({ dims: [1, 3], data: new Float32Array(3) }),
    { tokenizer: { model_max_length: 4096 }, async dispose() {} },
  );
  setTransformersJsRuntimeForTesting(async () => ({
    async pipeline(task, repo, options) {
      loads.push({ task, repo, options });
      return extractor;
    },
  }));
  t.after(() => setTransformersJsRuntimeForTesting(null));

  const model = new TransformersJsEmbeddingModel(entry(), {
    apiKey: "",
    llamaGpu: "metal",
  });
  await model.embed([{ kind: "text", text: "value" }]);

  assert.deepEqual(loads[0].options.session_options, {
    executionProviders: ["webgpu"],
  });
  await model.dispose();
});

test("Transformers.js adapter falls back to CPU when GPU initialization fails", async (t) => {
  const providers = [];
  const extractor = Object.assign(
    async () => ({ dims: [1, 3], data: new Float32Array(3) }),
    { tokenizer: { model_max_length: 4096 }, async dispose() {} },
  );
  setTransformersJsRuntimeForTesting(async () => ({
    async pipeline(_task, _repo, options) {
      const provider = options.session_options?.executionProviders[0];
      providers.push(provider);
      if (provider === "webgpu") {
        throw new Error("GPU unavailable");
      }
      return extractor;
    },
  }));
  t.after(() => setTransformersJsRuntimeForTesting(null));

  const writes = [];
  t.mock.method(process.stderr, "write", (message) => {
    writes.push(String(message));
    return true;
  });
  const model = new TransformersJsEmbeddingModel(entry(), {
    apiKey: "",
    llamaGpu: "metal",
  });
  await model.embed([{ kind: "text", text: "value" }]);

  assert.deepEqual(providers, ["webgpu", "cpu"]);
  assert.match(writes.join(""), /falling back to CPU/);
  await model.dispose();
});
