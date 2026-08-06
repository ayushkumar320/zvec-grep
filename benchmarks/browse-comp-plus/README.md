# BrowseComp-Plus

This standalone benchmark measures how zvec-grep affects agent quality,
retrieval behavior, and efficiency on the fixed BrowseComp-Plus knowledge
corpus.

The baseline uses the materialized Markdown corpus and its standard tools,
including raw `rg`. The treatment starts from an equivalent isolated agent
profile and is changed only by the official zvec-grep installer.

All orchestration is written in Python and runs through `uv`. The benchmark
uses `zg` and supported agents as external command-line interfaces, keeping it
independent of the zvec-grep implementation language.

## Setup

From the repository root:

```sh
cd benchmarks/browse-comp-plus
uv sync
source .venv/bin/activate
```

Generated corpora, indexes, credentials, and run artifacts are stored under
`benchmarks/browse-comp-plus/work/` and are not committed.

## Environment check

Set the credential used by the configured remote embedding provider, then run:

```sh
export DASHSCOPE_API_KEY="..."
zg-bench doctor
```

The command validates local dependencies, authentication, and disk capacity.
It writes a machine-readable report to `work/state/doctor.json` without
persisting credential values.

Use `zg-bench --help` for available options.
