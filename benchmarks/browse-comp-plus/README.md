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
zg-bench run --suite smoke --model <codex-model> --reasoning medium
```

Use the exact model ID exposed to the authenticated Codex account. The runner
validates cached model metadata before creating trials.

Run the fixed random 5-query CI subset:

```sh
zg-bench run --suite ci --model <codex-model> --reasoning medium
```

Run the first 80 cases in the pinned official dataset order for the study:

```sh
zg-bench run --suite study --model <codex-model> --reasoning medium
```

Run all 830 cases in the pinned official dataset:

```sh
zg-bench run --suite full --model <codex-model> --reasoning medium
```

## Evaluate and report

Export the format accepted by the official evaluator:

```sh
zg-bench evaluate <run-id> --evaluator official
```

Alternatively, create a manual paired-review sheet:

```sh
zg-bench evaluate <run-id> --evaluator manual
```

Enter scores from 0 to 1 in the generated sheet, then regenerate the report.
Retrieval recall is calculated directly from the dataset's official evidence
document IDs.

Regenerate the token, timing, completion, and paired-case report with:

```sh
zg-bench report <run-id>
```

## Artifacts

Generated data is stored under `artifacts/` and is not committed. It contains
the pinned source snapshots, materialized corpus, reusable index, run-local
isolated profiles, raw attempts, checkpoints, evaluator inputs, and reports.
Gold data and manifests remain outside the agent workspace.
