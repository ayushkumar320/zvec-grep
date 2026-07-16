import type { CodeSymbolType, ZvecGrepContextRoute } from "../index.js";

export type ColorMode = "auto" | "always" | "never";

export type PreviewMode = "none" | "short" | "full";

export type CliOptions = {
  mcp?: boolean;
  installTargets?: string[];
  installMcpToolTimeoutSeconds?: number;
  yes?: boolean;
  collection?: string;
  rg?: boolean;
  rgCompatibilityOptions?: string[];
  rgOptions?: CliRgOptions;
  rgPaths?: string[];
  debug?: boolean;
  trace?: boolean;
  human?: boolean;
  preview?: PreviewMode;
  color?: ColorMode;
  home?: string;
  embedding?: string;
  modelCacheDir?: string;
  llamaGpu?: "auto" | "metal" | "vulkan" | "cuda" | false;
  embeddingParallelism?: number;
  apiKey?: string;
  endpoint?: string;
  limit?: number;
  hybridQueries?: string[];
  routes?: ZvecGrepContextRoute[];
  fuse?: boolean;
  rebuild?: boolean;
  drop?: boolean;
  force?: boolean;
  resetPaths?: boolean;
  noAutoUpdate?: boolean;
  preferSymbol?: boolean;
  globs?: string[];
  insensitiveGlobs?: string[];
  fileTypes?: string[];
  excludedFileTypes?: string[];
  hidden?: boolean;
  noIgnore?: boolean;
  ignoreFiles?: string[];
  maxDepth?: number;
  maxFileSizeBytes?: number;
  follow?: boolean;
  modifiedAfter?: number;
  modifiedBefore?: number;
  symbolTypes?: CodeSymbolType[];
  embeddingConcurrency?: number;
};

export type CliCommand =
  | "query"
  | "index"
  | "status"
  | "collections"
  | "install"
  | "uninstall"
  | "serve"
  | "help"
  | "version";

export type CliRgOptions = {
  patterns?: string[];
  patternFiles?: string[];
  extraArgs?: string[];
  fixedStrings?: boolean;
  ignoreCase?: boolean;
  wordRegexp?: boolean;
  beforeContext?: number;
  afterContext?: number;
  hidden?: boolean;
};

export type ParsedArgs = {
  command: CliCommand;
  options: CliOptions;
  positionals: string[];
  helpTopic?: string;
};

export const DEFAULT_LIMIT = 10;

export const VALID_SYMBOL_TYPES = new Set<CodeSymbolType>([
  "module",
  "class",
  "interface",
  "function",
  "value",
  "alias",
]);
