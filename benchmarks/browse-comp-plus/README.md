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

## Setup

From this directory, install the Python environment and verify the host:

```sh
cd benchmarks/browse-comp-plus
uv sync
source .venv/bin/activate
zg-bench doctor
```

Codex must already be installed and authenticated. On macOS, the `codex` and
`rg` binaries bundled with the ChatGPT or Codex app are detected automatically
when they are not on `PATH`. The pinned `zg` 0.1.6 must be available on `PATH`.

## Prepare

Download the pinned official data, materialize every corpus `text` field
unchanged as `<docid>.md`, and build the reusable index:

```sh
zg-bench prepare
```

The first index build requires confirmation. For unattended setup, pass
`--yes`. Preparation is resumable and reuses an index whose corpus, model, and
runtime fingerprint matches the benchmark. Rebuilding a mismatched index always
requires an explicit command:

```sh
zg-bench index build --rebuild
```

Index output is streamed to the terminal and retained in
`artifacts/logs/index.stdout.log` and `index.stderr.log`.

The individual preparation stages are also available as `fetch`, `materialize`,
and `index build`.

## Run

Verify the complete paired workflow on one query:

```sh
zg-bench smoke --model <codex-model> --reasoning medium
```

Use the exact model ID exposed to the authenticated Codex account (for
example, `gpt-5.6-sol` rather than the family name `gpt-5.6`). The runner
validates cached model metadata before creating trials.

Run the fixed random 10-query CI subset:

```sh
zg-bench run --suite ci-10 --model <codex-model> --reasoning medium
```

Run the first 80 cases in the pinned official dataset order for the study:

```sh
zg-bench run --suite study-80 --model <codex-model> --reasoning medium
```

The CI sample is selected once from all 830 cases using the seed and SHA-256
procedure recorded in `suites/ci-10.txt`; it does not change between runs. Run
all 830 queries with `--suite full`. Pair order is deterministically
counterbalanced, every trial is persisted immediately, and a checkpoint report
is written after every ten completed pairs.

Both profiles run read-only against the same physical corpus root. The reusable
zvec-grep index is verified and warmed before measured trials; this setup time
is recorded separately in the run metadata.

At the start of each run, the runner creates run-local Codex profiles and
executes `zg install --target codex --yes` once for the treatment. It records the
selected zvec-grep build and profile fingerprints, then restarts the benchmark
daemon from that build. Every case in the run reuses those profile files. A
resumed run fails rather than changing treatment when its build or profile
fingerprint no longer matches.

Inspect or resume a run:

```sh
zg-bench status [run-id]
zg-bench inspect <run-id> --case <query-id> --profile zvec-grep
zg-bench resume <run-id>
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
