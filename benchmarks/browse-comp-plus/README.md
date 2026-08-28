<p align="right">
  English | <a href="./README_CN.md">中文</a>
</p>

# BrowseComp-Plus

This benchmark runs a native paired evaluation of Codex on the fixed
[BrowseComp-Plus](https://github.com/texttron/BrowseComp-Plus) corpus.

It follows the original paper's core principles, with small differences in
corpus processing and evaluation protocol to better reflect real-world use of a
general-purpose agent.

Each query is evaluated in independent paired trials with the same model,
prompt, corpus, Codex settings, and limits:

- **Baseline:** Codex with its standard set of tools.
- **zvec-grep:** the same Codex setup, with only the zvec-grep MCP tools and
  usage instructions added by `zg install`.

The benchmark records answer quality, token usage, wall-clock time, tool calls,
and complete Codex trajectories.

## Results

The [latest full report](./LATEST_REPORT.md) contains the complete results and
reproduction details. The sections below summarize the study: across 300 paired
trials, zvec-grep maintained answer quality while reducing average input tokens
by **37.56%**, tool calls by **43.52%**, and Agent time by **38.58%**.

### Study configuration

The study uses 100 cases to balance coverage, runtime, and cost. Cases are
selected in the pinned Hugging Face test-split order rather than sampled
randomly. We found no obvious ordering bias in this portion. Following a fixed
published order also minimizes discretion in selecting cases that might favor
zvec-grep.

Cases with verified errors or insufficient corpus evidence to determine the
answer are excluded. Each exclusion is documented in
[`suites/study.txt`](./suites/study.txt).

| Setting | Value |
| --- | --- |
| Evaluation scale | 100 cases · 300 paired trials |
| Agent | `gpt-5.6-sol` · `high` reasoning |
| Embedding model | `qwen/qwen3.7-text-embedding` |

### Primary results

All 300 paired trials are included in the averages. Changes show zvec-grep
relative to Baseline.

| Metric | Baseline | zvec-grep | Change |
| --- | ---: | ---: | ---: |
| Answer accuracy | 98.67% | 99.00% | +0.33 pp |
| Input tokens | 1.68M | 1.05M | **−37.56%** |
| Tool calls | 25.42 | 14.36 | **−43.52%** |
| Agent time | 259.4 s | 159.3 s | **−38.58%** |

zvec-grep index preparation is measured and reported separately from Agent
execution.

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

Run the fixed study subset:

```sh
zg-bench run --suite study
```

Run all cases in the pinned official dataset:

```sh
zg-bench run --suite full
```

## Evaluate and report

Evaluate the latest run with a blind Codex judge and generate its final report:

```sh
zg-bench evaluate
```

For the `smoke` suite only, evaluation also audits the zvec-grep profile's tool
trace and reports whether zvec-grep was used correctly. This audit is separate
from the blind answer-correctness judgement.

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
