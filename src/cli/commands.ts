import { resolve } from "node:path";
import {
  createZvecGrep,
  type CreateZvecGrepOptions,
  type IndexProgress,
  type RootPath,
  type ZvecGrepContextOptions,
} from "../index.js";
import { findNearestAnonymousWorkspace } from "../engine/service/root.js";
import type { ParsedArgs, CliOptions } from "./types.js";
import { contextWarningLines, printAgentContextResult, printHumanContextResult } from "./format/context.js";
import { printDebug } from "./format/debug.js";
import { createIndexProgressReporter } from "./format/progress.js";
import {
  printAnonymousInfo,
  printCollectionInfo,
  printCollectionList,
  printCollectionRemoveResult,
  printIndexPathFilterTip,
  printIndexResult,
} from "./format/status.js";


export async function runParsedCommand(parsed: ParsedArgs): Promise<void> {
  if (parsed.options.index) {
    await runIndex(parsed);
    return;
  }

  if (parsed.options.disableIndex) {
    await runDisableIndex(parsed);
    return;
  }

  if (parsed.options.status) {
    await runStatus(parsed);
    return;
  }

  if (parsed.options.collections) {
    await runCollections(parsed);
    return;
  }

  await runQuery(parsed);
}


async function runIndex(parsed: ParsedArgs): Promise<void> {
  const explicitRoot = parsed.positionals.length > 0;
  const root = resolveIndexRoot(parsed.positionals[0]);
  const rootPath = indexRootPath(root, parsed.options);
  if (parsed.positionals.length > 1) {
    throw new Error("zg --index accepts at most one root path");
  }

  const zvecGrep = await createZvecGrep(createServiceOptions(parsed.options, rootPath.absolutePath));
  const progress = createIndexProgressReporter();
  try {
    printIndexPathFilterTip(parsed.options);
    const result = await zvecGrep.index({
      root: rootPath.absolutePath,
      rootPaths: explicitRoot ? [rootPath] : undefined,
      rebuild: parsed.options.rebuild,
      resetPaths: parsed.options.resetPaths,
      includePaths: parsed.options.includePaths,
      excludePaths: parsed.options.excludePaths,
      embeddingConcurrency: parsed.options.embeddingConcurrency,
      onProgress: progress.report,
    });
    progress.finish();
    const info = await zvecGrep.info({ root: rootPath.absolutePath });
    printIndexResult("Indexed anonymous workspace", result, parsed.options, info.collection?.rootPaths);
  } catch (error) {
    progress.finish();
    throw error;
  } finally {
    await zvecGrep.close();
  }
}


async function runDisableIndex(parsed: ParsedArgs): Promise<void> {
  const root = resolveIndexRoot(parsed.positionals[0]);
  if (parsed.positionals.length > 1) {
    throw new Error("zg --disable-index accepts at most one root path");
  }

  const zvecGrep = await createZvecGrep(createServiceOptions(parsed.options, root));
  try {
    const info = await zvecGrep.disableIndex({ root });
    printAnonymousInfo(info, parsed.options);
  } finally {
    await zvecGrep.close();
  }
}


async function runStatus(parsed: ParsedArgs): Promise<void> {
  const root = parsed.positionals[0] ?? process.cwd();
  if (parsed.positionals.length > 1) {
    throw new Error("zg --status accepts at most one root path");
  }

  const zvecGrep = await createZvecGrep(createServiceOptions(parsed.options, root));
  try {
    const info = await zvecGrep.info({ root });
    printAnonymousInfo(info, parsed.options);
  } finally {
    await zvecGrep.close();
  }
}


async function runCollections(parsed: ParsedArgs): Promise<void> {
  const [action = "list", name, root] = parsed.positionals;
  const zvecGrep = await createZvecGrep(createServiceOptions(parsed.options, undefined));

  try {
    if (action === "list") {
      if (parsed.options.resetPaths) {
        throw new Error("--reset-paths can only be used with --collections index");
      }

      printCollectionList(await zvecGrep.collections.list(), parsed.options);
      return;
    }

    if (action === "info") {
      if (parsed.options.resetPaths) {
        throw new Error("--reset-paths can only be used with --collections index");
      }

      if (!name) {
        throw new Error("zg --collections info requires <name>");
      }

      const [info, status] = await Promise.all([
        zvecGrep.collections.info(name),
        zvecGrep.collections.status(name),
      ]);
      if (!info) {
        throw new Error(`Collection not found: ${name}`);
      }

      printCollectionInfo(info, status, parsed.options);
      return;
    }

    if (action === "index") {
      if (!name) {
        throw new Error("zg --collections index requires <name>");
      }

      const explicitRoot = root !== undefined;
      const rootPath = indexRootPath(root ?? process.cwd(), parsed.options);
      const rootPaths = explicitRoot ? rootPath : undefined;
      const progress = createIndexProgressReporter();
      try {
        const result = await zvecGrep.collections.index(name, rootPaths, {
          rebuild: parsed.options.rebuild,
          resetPaths: parsed.options.resetPaths,
          includePaths: parsed.options.includePaths,
          excludePaths: parsed.options.excludePaths,
          embeddingConcurrency: parsed.options.embeddingConcurrency,
          onProgress: progress.report,
        });
        progress.finish();
        const info = await zvecGrep.collections.info(name);
        printIndexResult(`Indexed collection ${name}`, result, parsed.options, info?.rootPaths);
      } catch (error) {
        progress.finish();
        throw error;
      }
      return;
    }

    if (action === "remove") {
      if (parsed.options.resetPaths) {
        throw new Error("--reset-paths can only be used with --collections index");
      }

      if (!name) {
        throw new Error("zg --collections remove requires <name>");
      }

      const removed = await zvecGrep.collections.remove(name);
      printCollectionRemoveResult(name, removed, parsed.options);
      return;
    }

    if (parsed.options.resetPaths) {
      throw new Error("--reset-paths can only be used with --collections index");
    }

    throw new Error(`Unknown collections action: ${action}`);
  } finally {
    await zvecGrep.close();
  }
}


async function runQuery(parsed: ParsedArgs): Promise<void> {
  const rgInput = parsed.options.rg
    ? normalizeRgInput(parsed)
    : undefined;
  const commandOptions = rgInput?.options ?? parsed.options;
  const queries = (rgInput?.queries ?? parsed.positionals)
    .map((query) => query.trim())
    .filter((query) => query.length > 0);
  const routes = parsed.options.routes ?? [];
  if (queries.length === 0 && routes.length === 0) {
    throw new Error(parsed.options.rg
      ? "zg --rg requires a pattern. Use --help for examples."
      : "zg query requires text or --fts/--vector routes. Use --help for examples.");
  }

  const zvecGrep = await createZvecGrep(createServiceOptions(commandOptions, undefined));
  const progress = createIndexProgressReporter();
  try {
    const result = await zvecGrep.context(contextOptions(commandOptions, queries, (progressEvent) => {
      if (progressEvent.phase !== "done") {
        progress.report(progressEvent);
      }
    }));
    progress.finish();
    if (commandOptions.human) {
      printHumanContextResult(result, commandOptions);
    } else {
      printAgentContextResult(result, commandOptions);
    }
    for (const line of contextWarningLines(result)) {
      console.error(line);
    }

    if (commandOptions.debug) {
      printDebug(result, {
        trace: commandOptions.trace === true,
      });
    }
  } catch (error) {
    progress.finish();
    throw error;
  } finally {
    await zvecGrep.close();
  }
}


function contextOptions(
  options: CliOptions,
  queries: readonly string[],
  onAutoUpdateProgress?: (progress: IndexProgress) => void,
): ZvecGrepContextOptions {
  return {
    queries: queries.length > 0 ? queries : undefined,
    rg: options.rg,
    rgOptions: options.rgOptions,
    rgPaths: options.rgPaths,
    routes: options.routes,
    collection: options.collection,
    limit: options.limit,
    fallback: "disabled",
    autoUpdate: !options.noAutoUpdate,
    onAutoUpdateProgress,
    trace: options.trace,
    preferSymbol: options.preferSymbol,
    includePaths: options.includePaths,
    excludePaths: options.excludePaths,
    modifiedAfter: options.modifiedAfter,
    modifiedBefore: options.modifiedBefore,
    symbolTypes: options.symbolTypes,
    embeddingConcurrency: options.embeddingConcurrency,
  };
}


function normalizeRgInput(parsed: ParsedArgs): { queries: string[]; options: CliOptions; } {
  const explicitPatterns = parsed.options.rgOptions?.patterns ?? [];
  const queries = explicitPatterns.length > 0
    ? explicitPatterns
    : parsed.positionals.slice(0, 1);
  const paths = explicitPatterns.length > 0
    ? parsed.positionals
    : parsed.positionals.slice(1);

  return {
    queries,
    options: {
      ...parsed.options,
      rgPaths: paths.length > 0 ? paths : undefined,
    },
  };
}


function createServiceOptions(
  options: CliOptions,
  root: string | undefined,
): CreateZvecGrepOptions {
  const embedding = options.embedding ?? process.env.ZVEC_GREP_EMBEDDING;
  const apiKey = options.apiKey
    ?? process.env.ZVEC_GREP_API_KEY
    ?? process.env.DASHSCOPE_API_KEY
    ?? process.env.QWEN_API_KEY;
  const endpoint = options.endpoint ?? process.env.ZVEC_GREP_ENDPOINT;

  return {
    root,
    home: options.home ?? process.env.ZVEC_GREP_HOME,
    embedding,
    apiKey,
    endpoint,
    modelCacheDir: options.modelCacheDir ?? process.env.ZVEC_GREP_MODEL_CACHE,
    llamaGpu: options.llamaGpu ?? parseEnvLlamaGpu(process.env.ZVEC_GREP_LLAMA_GPU),
    embeddingParallelism: options.embeddingParallelism ?? parseEnvPositiveInteger(process.env.ZVEC_GREP_EMBED_PARALLELISM),
  };
}


function parseEnvLlamaGpu(value: string | undefined): CreateZvecGrepOptions["llamaGpu"] | undefined {
  const normalized = value?.trim().toLowerCase() ?? "";
  if (!normalized) {
    return undefined;
  }

  if (normalized === "auto" || normalized === "metal" || normalized === "vulkan" || normalized === "cuda") {
    return normalized;
  }

  if (["false", "off", "none", "disable", "disabled", "0"].includes(normalized)) {
    return false;
  }

  return undefined;
}


function parseEnvPositiveInteger(value: string | undefined): number | undefined {
  const normalized = value?.trim() ?? "";
  if (!normalized) {
    return undefined;
  }

  const parsed = Number.parseInt(normalized, 10);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : undefined;
}


function resolveIndexRoot(root: string | undefined): string {
  if (root !== undefined) {
    return root;
  }

  return findNearestAnonymousWorkspace(process.cwd())?.root ?? process.cwd();
}


function indexRootPath(path: string, options: CliOptions): RootPath {
  return {
    absolutePath: resolve(path),
    recursive: true,
    include: options.includePaths,
    exclude: options.excludePaths,
  };
}
