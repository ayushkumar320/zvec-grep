# Benchmark skills

The `zvec-grep` skill is a temporary benchmark integration shim. Once
`zg install` can configure the selected agent inside Harbor's isolated task
environment, the benchmark should use that command and remove this local skill.

The runner will continue to build the task index separately because indexing
represents user-owned setup rather than agent behavior.
