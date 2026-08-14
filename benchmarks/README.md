# Benchmarks

Each benchmark is maintained as a self-contained project in a dedicated
subdirectory. Its README describes the methodology, dependencies, runner, setup,
execution, and generated artifacts.

## Benchmark Suites

- [`browse-comp-plus/`](browse-comp-plus/README.md): knowledge-base retrieval
  and end-to-end answer evaluation on BrowseComp-Plus.
- [`coding/`](coding/README.md): paired OpenCode evaluations with and without
  zvec-grep on a pinned 20-task SWE-QA-Bench subset, using an automated
  five-task scope and manually triggered smoke or all-task scopes.

## What We Measure

Our agent benchmarks use paired experiments. The baseline and treatment use the
same task, dataset, model, agent, environment, and limits.

- **Baseline:** uses the agent's default tools and instructions.
- **Treatment:** additionally includes **zvec-grep** and its standard usage
  instructions (this integration is the only intended difference between the two
  conditions).

We record, where applicable:

- result quality;
- token usage;
- cost;
- wall-clock time;
- tool-call counts.

> **Note:** Index construction time and resources are reported separately from
> agent or query execution so one-time setup cost is not confused with
> steady-state use.
