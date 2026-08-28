# Model-layer benchmark

This compares the pinned `local/potion-code-16m-v2` implementation in
`origin/main` with the Rust implementation. Downloads are completed before the
timed rounds. Both harnesses use the same generated texts, batch size, request
concurrency, warmup count and vector count.

The Rust harness passes request concurrency into `ModelRuntimeRequest`, so the
measurement exercises the same operation-level limit exposed by
`embeddingConcurrency` rather than bypassing the runtime manager.

The Rust harness is the ignored `model_layer_throughput` unit test. Build it in
release mode and run the resulting `zg_engine-*` test executable directly. The
TypeScript harness expects an `origin/main` worktree that has been built with
`npm run build`.

`measure.py -- <command>` samples process RSS every 10 ms and prints peak RSS.
Run a baseline process for each implementation, then subtract it from the model
run when comparing incremental model memory. Use separate processes for every
configuration and repeat measurements to detect noise.

`compare.py` automates the comparison and reports medians across independent
processes. It reports both resident RSS immediately after warmup and peak RSS
during the throughput rounds. For example:

```sh
python3 benchmarks/model-layer/compare.py \
  --rust-bin target/release/deps/zg_engine-<hash> \
  --main-root /path/to/origin-main-worktree \
  --cache ~/.zvec-grep/models
```
