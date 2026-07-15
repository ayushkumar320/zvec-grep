# Benchmarks

This directory contains benchmarks for measuring how `zvec-grep` affects an
agent's ability to complete real-world tasks.

The comparison keeps the model, agent loop, task prompt, environment, and limits
the same. The only difference is the tool profile:

- **Baseline:** the agent's standard tools, including shell utilities such as
  `rg` and `find` when available.
- **zvec-grep:** the same tools, plus `zvec-grep` and usage guidance such as a
  skill Markdown file.

## Benchmark suites

- **[SWE-bench Verified](https://www.swebench.com/SWE-bench/guides/datasets/):**
  evaluates an agent's ability to resolve real-world software issues by
  modifying an existing repository. Solutions are graded by running repository
  tests.
- **[Terminal-Bench 2.1](https://www.tbench.ai/news/terminal-bench-2-1):** a
  collection of complex tasks completed in isolated terminal environments. It
  covers areas such as software engineering, system administration, data
  processing, and machine learning, with programmatic evaluation of results.

## Run tiers

Each benchmark suite can be run at different tiers:

- **Smoke:** one task that quickly verifies the complete benchmark workflow.
- **CI:** a fixed, representative subset used to detect regressions over time.
- **Full:** the complete suite, used for release results and reports.

Smoke and CI results help us develop and maintain the benchmark. Only full runs
are intended to support general performance claims.

## Metrics

We track two outcomes and one diagnostic:

1. **Outcome quality:** the score or reward from the benchmark's official
   evaluator, including correctness and any benchmark-provided quality signals.
2. **Efficiency:** tokens, wall-clock time, cost, and tool calls used to
   complete the task.
3. **Tool behavior:** whether and how the agent used `zvec-grep`, and how
   quickly it reached relevant information.

## Instructions

### Setup

You need [uv](https://docs.astral.sh/uv/), Docker Engine or Docker Desktop, and
the credentials required by your chosen Harbor agent and model. From this
directory, install the pinned environment and check the local setup:

```sh
uv sync
uv run zg-bench doctor
```

### Run the smoke test

Run either baseline smoke task with the same agent and model:

```sh
uv run zg-bench run swebench-verified \
  --agent <harbor-agent> --model <provider/model>

uv run zg-bench run terminal-bench-2.1 \
  --agent <harbor-agent> --model <provider/model>
```

Add `--dry-run` to inspect the generated Harbor command without starting a
container. Harbor writes trajectories and evaluator output to `runs/`.

### Run the CI test

The CI tier is not implemented yet. It will run a fixed, representative task
set for each benchmark suite.

### Run the full benchmark

The Full tier is not implemented yet. It will run the complete task set for a
benchmark suite and produce the results used in external reports.

Smoke tests can run through Docker on Linux or macOS. On Apple silicon, some
benchmark images may use emulation; full benchmark reports should use a
consistent Linux x86-64 environment.
