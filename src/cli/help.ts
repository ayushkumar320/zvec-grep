export function printHelp(version: string, topic?: string): void {
  if (!topic) {
    console.log(mainHelp(version));
    return;
  }

  const help = commandHelp(topic);
  if (!help) {
    throw new Error(`Unknown help topic: ${topic}`);
  }
  console.log(help);
}

function mainHelp(version: string): string {
  return `zvec-grep ${version}

Usage:
  zg <command> [options]

Commands:
  query          Search indexed context or run managed ripgrep
  index          Build, rebuild, or drop the workspace index
  status         Show workspace and index status
  collections    Manage named collections
  install        Install agent integrations
  uninstall      Remove agent integrations
  help           Show help for a command
  version        Print the installed version

Examples:
  zg query "where authentication is validated"
  zg query --fts "AuthService"
  zg query --rg -F "AuthService" src
  zg index --embedding local/embeddinggemma-300m
  zg status
  zg install

Run zg help <command> or zg <command> --help for command-specific help.
Use zg -h/--help for this page and zg -v/--version for the version.`;
}

function commandHelp(topic: string): string | undefined {
  switch (topic) {
    case "query":
      return `Usage:
  zg query <query> [options]
  zg query --hybrid <query> --fts <query> --vector <query> [--fuse]
  zg query --rg [rg-options] <pattern> [path...]

Search routes:
  positional query                  Hybrid FTS and vector search
  --hybrid <query>                  Add an explicit hybrid query
  --fts <query>                     Add an exact/lexical query
  --vector <query>                  Add a semantic/vector query
  --fuse                            Fuse all query groups into one ranked list
  --rg                              Run exhaustive managed ripgrep; cannot be mixed with indexed routes

Each route option consumes one query and may be repeated. Without --fuse,
--limit applies to each query group; with --fuse it applies to the final list.

Result options:
  --limit <n>                       Maximum results
  --human                           Human-readable ranked output
  --preview <none|short|full>       Indexed source preview size
  --debug                           Print diagnostics to stderr
  --trace                           Include per-hit indexed search trace
  --color <auto|always|never>       Color mode
  --no-color                        Disable color

File filters (indexed and --rg):
  -g, --glob <glob>                 Include paths; prefix with ! to exclude; repeatable
  --iglob <glob>                    Case-insensitive path glob; repeatable
  -t, --type <type>                 Include a ripgrep file type; repeatable
  -T, --type-not <type>             Exclude a ripgrep file type; repeatable
  --modified-after <time>           Only files modified after a date or epoch milliseconds
  --modified-before <time>          Only files modified before a date or epoch milliseconds

Indexed-only filters:
  --collection <name>               Query a named collection
  --symbol-type <type>              module, class, interface, function, value, alias
  --prefer-symbol                   Prefer exact indexed symbols
  --no-auto-update                  Do not refresh a stale anonymous index
  --embedding-concurrency <n>       Concurrency for automatic index refresh

Managed --rg supports common ripgrep matching, file-selection, context, engine,
encoding, and discovery flags, including -e/-f, -F, -i/-s/-S, -w/-x/-v,
-A/-B/-C, -m, -P/-U, -g/--iglob, -t/-T, --hidden, --no-ignore,
--ignore-file, --max-depth, --max-filesize, -L, and -z. Use -e when a
pattern begins with "-". Options that replace rg's output format, such as
--json, --count, --files, -l, -o, --replace, and --vimgrep, are rejected.
.git/** and .zvec-grep/** remain excluded.`;
    case "index":
      return `Usage:
  zg index [root] [options]
  zg index [root] --rebuild [options]
  zg index [root] --drop [--yes]

Options:
  --embedding <model>               Model such as local/embeddinggemma-300m or qwen/qwen3.7-text-embedding
  --rebuild                         Rebuild the existing index
  --drop                            Permanently remove the workspace index
  --yes                             Confirm --drop without prompting
  -g, --glob <glob>                 Include paths; prefix with ! to exclude; repeatable
  --iglob <glob>                    Case-insensitive path glob; repeatable
  -t, --type <type>                 Include a ripgrep file type; repeatable
  -T, --type-not <type>             Exclude a ripgrep file type; repeatable
  --hidden                          Include hidden paths except .git and .zvec-grep
  --no-ignore                       Do not apply default or .gitignore rules
  --ignore-file <path>              Add an explicit ignore file; repeatable
  --max-depth <n>                   Maximum directory depth
  --max-filesize <size>             Maximum bytes or K/M/G/T size
  -L, --follow                      Follow symbolic links safely
  --reset-paths                     Clear inherited file-selection settings
  --embedding-concurrency <n>       Embedding task concurrency
  --api-key <key>                   Embedding provider API key
  --endpoint <url>                  Embedding provider endpoint
  --model-cache <path>              Local model cache directory
  --gpu                             Try GPU acceleration
  --no-gpu                          Force CPU local embeddings
  --llama-gpu <mode>                auto, metal, vulkan, cuda, off
  --embedding-parallelism <n>       Local embedding context parallelism

New indexes require --embedding, ZVEC_GREP_EMBEDDING, or a configured default.
Existing indexes reuse their stored embedding schema. Explicit model/provider
options used for a successful index are persisted in ~/.zvec-grep/config.json.`;
    case "status":
      return `Usage:
  zg status [root] [--color <auto|always|never>] [--no-color]

Shows the nearest workspace root, index policy, index state, embedding schema,
stored paths, refresh status, and suggested next action.`;
    case "collections":
      return `Usage:
  zg collections
  zg collections info <name>
  zg collections index <name> [root] [options]
  zg collections remove <name>

Named collections support the same embedding, file-selection, discovery,
rebuild, and embedding-concurrency options as zg index.`;
    case "install":
      return `Usage:
  zg install [--target codex|all|auto] [--yes] [--force]

Options:
  --target <agent>                  Agent integration to install
  --mcp-tool-timeout <seconds>      MCP tool timeout written to agent configuration
  --yes                             Use default choices without prompting
  --force                           Replace conflicting unmanaged integration configuration

This installs agent guidance and MCP configuration. It does not install the npm package.`;
    case "uninstall":
      return `Usage:
  zg uninstall [--target codex|all|auto] [--yes]

Removes zvec-grep-managed agent guidance and MCP configuration. It does not
remove the npm package or unrelated user configuration.`;
    case "help":
      return `Usage:
  zg help [command]
  zg <command> --help
  zg -h
  zg --help`;
    case "version":
      return `Usage:
  zg version
  zg version -v
  zg -v
  zg --version`;
    case "serve":
      return `Usage:
  zg serve --mcp

Starts the stdio MCP process used by installed agent integrations.`;
    default:
      return undefined;
  }
}
