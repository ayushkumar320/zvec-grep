# Benchmarks

Benchmark fixtures and baselines live here. Every benchmark records cold/warm
state, Direct/Server mode, p50/p95 wall time, CPU time, peak RSS and result
quality. The first baseline compares current TypeScript and Rust
`zg query --rg` against an optional standalone `rg` oracle and the same corpus.
The product binary itself uses embedded `grep`/`ignore` crates and does not
require `rg` to be installed.
