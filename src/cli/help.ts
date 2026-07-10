export function printHelp(version: string): void {
  console.log(`zvec-grep ${version}

Usage:
  zg [options] <query...>
  zg "multi word query" "another query"
  zg --rg "Symbol|LogMessage"
  zg --rg -F "ExactSymbolOrText" src
  zg <query...> --fts <query...> --vector <query...>
  zg --fts <query...>
  zg --human [options] <query...>
  zg --index --embedding <model> [root]
  zg --index --embedding <model> --include "src/**" --exclude "dist/**,node_modules/**" [root]
  zg --index --embedding local/embeddinggemma-300m [root]
  zg --disable-index [root]
  zg --status [root]
  zg --collection <name> [options] <query...>
  zg --collections
  zg --collections info <name>
  zg --collections index <name> [root]
  zg --collections remove <name>

Output:
  default                         Agent markdown grouped by file
  --human                         Human-readable ranked output
  --preview <none|short|full>     Indexed result preview size (default none; --human default full)
  default --rg output             path:range plus numbered lines; no source label
                                  match lines use <line>:, context lines use <line>-

Query:
  Each shell argument is one hybrid query. Quote multi-word queries.
  Without --limit, 1-3 query groups return up to 10 each; more groups share about 30.
  Default indexed queries require an indexed anonymous workspace. Run zg --index first.
  Use --disable-index to remember that this workspace should not be indexed.
  Existing anonymous indexes are checked and incrementally refreshed before query.
  Indexing skips dependency/third-party/generated/build/cache dirs, hidden paths, nested git repos, .gitignore entries, and files over 1 MB by default.
  New indexes require --embedding <model> or ZVEC_GREP_EMBEDDING; existing indexes reuse stored schema.
  --rg is explicit exhaustive ripgrep search and works without an index.
  Without --limit, --rg returns every match.
  --rg uses rg regex syntax by default. Use -F/--fixed-strings for literal text.
  Use -e/--regexp when the pattern begins with "-".
  --rg accepts common agent rg flags: -n, -H, -F, -i, -w, -A/-B/-C,
  -e/--regexp, -g/--glob, --hidden, -t/--type, -T/--type-not,
  --max-depth, --ignore-file, --no-ignore, --smart-case, and --pcre2.

Options:
  --debug                         Print diagnostics to stderr
  --trace                         Print inline per-hit search trace
  --status                        Show anonymous workspace/index status for this root
  --color <auto|always|never>     Colorize human/status output (default auto)
  --no-color                      Disable color output
  --limit <n>                     Maximum returned items per query/group
  --preview <none|short|full>     Control indexed source preview; not used with --rg
  --rg                            Managed ripgrep search with zvec-grep output
  --disable-index                 Mark this anonymous workspace as no-index
  --fts <query...>                Add exact/lexical search routes
  --vector <query...>             Add semantic/vector search routes
  --no-auto-update                Do not refresh an existing stale anonymous index before query
  --no-fallback                   Compatibility flag; anonymous queries already require an index
  --collection <name>             Query a named collection
  --collections                   Manage named collections
  --home <path>                   Named collection registry home
  --embedding <model>             Embedding model, e.g. local/embeddinggemma-300m or qwen/text-embedding-v4
  --model-cache <path>            Local model cache directory
  --gpu                           Try GPU acceleration; falls back to CPU if unavailable
  --no-gpu                        Force CPU local embeddings
  --llama-gpu <mode>              CPU by default; auto, metal, vulkan, cuda, off
  --embedding-parallelism <n>     Local embedding context parallelism
  --api-key <key>                 Embedding provider API key
  --endpoint <url>                Embedding provider endpoint
  --include <glob>                Include indexed/query paths
  --exclude <glob>                Exclude indexed/query paths
  --modified-after <time>         Query files modified after time
  --modified-before <time>        Query files modified before time
  --symbol-type <type>            module, class, interface, function, value, alias
  --prefer-symbol                 Prefer exact indexed symbols
  --rebuild                       Rebuild on --index/collections index
  --reset-paths                   Clear inherited include/exclude filters on rebuild

Environment:
  ZVEC_GREP_HOME
  ZVEC_GREP_EMBEDDING
  ZVEC_GREP_MODEL_CACHE
  ZVEC_GREP_LLAMA_GPU
  ZVEC_GREP_EMBED_PARALLELISM
  ZVEC_GREP_API_KEY
  ZVEC_GREP_ENDPOINT
  NO_COLOR
`);
}
