# BrowseComp-Plus

This benchmark runs a native paired evaluation of Codex on the fixed
[BrowseComp-Plus](https://github.com/texttron/BrowseComp-Plus) corpus.

Each query is run twice with the same model, prompt, corpus, Codex settings, and
limits:

- **Baseline:** Codex with its standard set of tools.
- **zvec-grep:** the same Codex setup, with only the zvec-grep MCP tools and
  usage instructions added by `zg install`.

The benchmark records answer quality, token usage, wall-clock time, tool calls,
and complete Codex trajectories.

## Prerequisites

From this directory, install the Python environment and verify the host:

```sh
cd benchmarks/browse-comp-plus
uv sync
source .venv/bin/activate
zg-bench doctor
```

The host environment should provide:

- macOS or Linux with `uv`;
- an installed and authenticated Codex CLI;
- `zg` installed.

## Prepare the benchmark

Download the pinned official data, materialize every corpus `text` field
unchanged as `<docid>.md`, and build the reusable index:

```sh
zg-bench prepare
```

Initial preparation requires network access and sufficient disk space for the
downloaded data, materialized corpus, and index.

Subsequent runs reuse completed download, corpus, and index stages.

## Run

Verify the complete paired workflow on one query:

```sh
zg-bench run --suite smoke
```

The Codex model and reasoning effort are configured in `benchmark.toml`. The
runner validates the configured model before creating trials.

Run the fixed random 5-query CI subset:

```sh
zg-bench run --suite ci
```

Run the first 80 cases in the pinned official dataset order for the study:

```sh
zg-bench run --suite study
```

Run all 830 cases in the pinned official dataset:

```sh
zg-bench run --suite full
```

## Evaluate and report

Evaluate the latest run with a blind Codex judge and generate its final report:

```sh
zg-bench evaluate
```

Specify a run explicitly when needed:

```sh
zg-bench evaluate <run-id>
```

Regenerate the latest run's token, timing, completion, and paired-case report:

```sh
zg-bench report
```

Specify a run explicitly when needed:

```sh
zg-bench report <run-id>
```

Delete all runs and generated reports while preserving the downloaded data,
workspaces, and reusable index:

```sh
zg-bench clean
```

## Artifacts

Generated data is stored under `artifacts/` and is not committed. It contains
the pinned source snapshots, materialized corpus, reusable index, run-local
isolated profiles, raw attempts, evaluator inputs, and reports. Gold data and
manifests remain outside the agent workspace.
