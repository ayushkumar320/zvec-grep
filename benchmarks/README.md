<p align="right">
  English | <a href="./README_CN.md">中文</a>
</p>

# Benchmarks

Each evaluation is **reproducible by design** and measures how zvec-grep
affects agent answer quality and retrieval efficiency across different
workloads. Every benchmark is self-contained, with **pinned inputs and
dependencies**, its own runner and evaluation workflow, a clear
generated-artifact boundary, and a detailed README.

See benchmark-specific README for setup and execution instructions.

## Benchmark suites

| Benchmark | Description | Agent | Study scope |
| --- | --- | --- | --- |
| [BrowseComp-Plus](browse-comp-plus/README.md) | Evaluates multi-document evidence retrieval and answer accuracy over a large, fixed corpus | Codex | 80 cases |
| [SWE-QA-Bench](swe-qa-bench/README.md) | Evaluates repository-level, cross-file, and multi-hop software-engineering question answering | OpenCode | 20 tasks |

## Evaluation protocol

All benchmarks use controlled, paired A/B evaluations. For each case, the
baseline and treatment profiles keep the task inputs, agent, model, environment,
and limits fixed.

- **Baseline:** the agent uses its standard tools and instructions.
- **Treatment (zvec-grep):** the same agent additionally receives a prepared
  index, zvec-grep tools, and standard usage guidance.

The **only intended difference** between paired runs is access to zvec-grep.
To keep the comparison focused on agent behavior, index preparation is measured
and reported separately.

## Evaluation metrics

Where applicable, benchmarks measure:

| Metric | What it measures | Better |
| --- | --- | --- |
| Answer quality | Task-specific judge score or accuracy | Higher |
| Input tokens | Model input consumed during agent execution | Lower |
| Tool calls | Recorded tool invocations during agent execution | Lower |
| Agent wall time | Agent execution time, excluding separately reported zvec-grep index preparation time | Lower |

Additional metrics may be reported when relevant. Completion status and raw
trajectories may also be retained for auditing and diagnosis.

## Reproducibility

Compare baseline and zvec-grep only within the same paired run. Results produced
with different models, environments, dataset revisions, repository commits, or
index configurations are not directly comparable. Use each benchmark's locked
dependencies and documented preparation path when reproducing results.
