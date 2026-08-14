# Benchmarks

This directory contains the reproducible benchmark projects used to measure how
`zvec-grep` changes agent quality and efficiency. Each project is self-contained:
it has its own locked environment, runner, dataset preparation, evaluation
workflow, generated artifacts, and detailed README.

Run each benchmark's local `uv` and `zg-bench` commands from its own directory.
Follow the benchmark-specific README for setup or workflow commands that must
run from the repository root.

## At a glance

| Benchmark | What it measures | Agent and runner | Pinned scope | Canonical entry point |
| --- | --- | --- | --- | --- |
| [BrowseComp-Plus](browse-comp-plus/README.md) | Knowledge-base retrieval and end-to-end answer quality over a fixed corpus | Codex with the native paired runner | Smoke: 1 query; CI: 5; study: 80; full: 830 | `zg-bench prepare`, then `zg-bench run --suite <suite>` |
| [SWE-QA-Bench](swe-qa-bench/README.md) | Repository-level software-engineering question answering | OpenCode and Harbor, orchestrated by GitHub Actions | Automatic: 5 tasks; manual smoke: 1; manual all-full: 20 | [`swe-qa-bench.yml`](../.github/workflows/swe-qa-bench.yml) or the local `zg-bench` runner |

## Shared methodology

Both projects use paired experiments. For a given case, the baseline and
treatment keep the model, task inputs, agent settings, environment, and limits
fixed.

- **Baseline:** the agent uses its standard tools and instructions.
- **zvec-grep:** the same agent additionally receives the zvec-grep tools,
  standard usage guidance, and a prepared index.

The intended experimental variable is access to zvec-grep. Index preparation is
reported separately from agent or query execution so one-time setup cost is not
mixed with steady-state behavior.

The projects record the metrics applicable to their runners:

- judged answer quality;
- input-token usage;
- tool-call counts;
- wall-clock time;
- completion state and raw trajectories.

Higher quality is better, while fewer tokens, tool calls, and seconds indicate
an efficiency improvement. Refer to each benchmark README for its exact judge,
aggregation, and change-sign conventions.

## BrowseComp-Plus

[BrowseComp-Plus](browse-comp-plus/README.md) evaluates Codex against the pinned
official BrowseComp-Plus corpus. Preparation downloads the source data,
materializes every corpus document unchanged, and builds a reusable index. Each
query then runs once with the baseline profile and once with the zvec-grep
profile.

The runner supports four fixed suites:

- `smoke`: one query for validating the complete paired workflow;
- `ci`: a fixed random five-query subset;
- `study`: the first 80 official cases;
- `full`: all 830 official cases.

A blind Codex judge evaluates answers after a run. Reports include paired-case
quality, token, timing, tool-call, completion, and trajectory data. Generated
snapshots, indexes, runs, and reports live under
`benchmarks/browse-comp-plus/artifacts/` and are not committed.

See the [BrowseComp-Plus README](browse-comp-plus/README.md) for prerequisites,
data preparation, execution, evaluation, reporting, and cleanup commands.

## SWE-QA-Bench

[SWE-QA-Bench](swe-qa-bench/README.md) evaluates repository-level answers on a
locked 20-task subset of `peng-weihan/SWE-QA-Bench`. Every task pins its source
repository commit, Harbor environment, prompt, verifier, and isolated judge
reference.

The GitHub Actions workflow provides these scopes:

- same-repository pull requests and pushes to `main`: five curated tasks;
- fork and Dependabot pull requests: validation, tests, and dry-run only;
- manual `scope=smoke`: one task;
- manual `scope=all-full`: all 20 tasks.

Each selected task runs three baseline trials and three zvec-grep trials on the
same runner, and all six answers are judged independently. The workflow is
report-only: numeric results do not gate review or merging, but all expected
profile runs and judge calls must complete.

Task reports and the Aggregate row display `baseline / zvec-grep / change`.
Judge change is a score delta, while token, tool-call, and agent-time changes are
relative efficiency changes. Aggregate efficiency changes are equal-weight
means of task-level changes, not ratios of summed totals.

See the [SWE-QA-Bench README](swe-qa-bench/README.md) for the locked selection,
CI scopes, exact metric formulas, local setup, dry-run command, and failure
diagnostics.

## Reproducibility

Do not compare results produced with different models, limits, operating
systems, corpus revisions, repository commits, or index configurations. Use the
locked dependencies and benchmark-specific preparation path, and keep generated
artifacts outside agent-visible workspaces so references and prior answers
cannot leak into a run.
