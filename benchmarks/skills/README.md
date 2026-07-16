# Benchmark skills

The `zvec-grep` skill is a temporary benchmark integration shim. Although
`zg install` can configure Codex, the benchmark currently injects this pinned
skill so that Codex and Qwen Code receive the same reproducible guidance. Once
all supported benchmark agents can be provisioned consistently through
`zg install`, this local skill should be removed.

The runner will continue to build the task index separately because indexing
represents user-owned setup rather than agent behavior.
