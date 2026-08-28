# Model-layer benchmark results

Date: 2026-08-28

## Scope

- Rust: `dev/rust` at `9f81a0c`, plus the working-tree controlled compute
  runtime
- TypeScript: `origin/main` at `d41ee79`
- Model: `local/potion-code-16m-v2`
- Revision: `e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b`
- Machine: Apple M4 Pro, 12 physical/logical cores, 48 GiB RAM
- Toolchains: Rust 1.98.0 release build; Node.js 25.5.0

Both implementations used the same cached model files and generated text batch.
Downloads and model warmup were excluded from throughput timing. Each result is
the median of three independent processes. Each process ran two warmup waves and
five measured rounds of 16,384 vectors with batch size 256.

For Rust, each concurrency value was passed to `ModelRuntimeRequest` as the
operation's embedding concurrency. Each batch occupies one operation permit and
one job in the process-level bounded compute pool. Tokenization inside a batch
is serial, matching the task-concurrency meaning of the user option.

RSS was sampled every 10 ms. `loaded RSS` is the process resident set after
warmup and before timed rounds. `peak RSS` is the maximum during the complete
throughput workload.

## Results

| Concurrency | Rust vectors/s | main vectors/s | Rust throughput vs main |
| ---: | ---: | ---: | ---: |
| 1 | 41,705 | 25,793 | +61.7% |
| 2 | 55,409 | 47,095 | +17.7% |
| 4 | 99,769 | 77,999 | +27.9% |

| Concurrency | Rust loaded RSS | main loaded RSS | Rust loaded RSS vs main | Rust peak RSS | main peak RSS | Rust peak RSS vs main |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 132.58 MiB | 145.88 MiB | 9.1% lower | 132.61 MiB | 317.52 MiB | 58.2% lower |
| 2 | 132.95 MiB | 187.48 MiB | 29.1% lower | 133.12 MiB | 409.88 MiB | 67.5% lower |
| 4 | 135.20 MiB | 265.91 MiB | 49.2% lower | 135.56 MiB | 481.06 MiB | 71.8% lower |

The output checksum was exactly `3773.970746099949` in every measured process
for both implementations.

## Memory attribution

The paired no-model process baselines were 7.58–7.61 MiB for the Rust test
executable and 74.98–75.08 MiB for Node after importing the main package.
Subtracting each configuration's paired median baseline gives the following
warm model increments:

| Implementation | Concurrency 1 | Concurrency 2 | Concurrency 4 |
| --- | ---: | ---: | ---: |
| Rust | 124.97 MiB | 125.34 MiB | 127.59 MiB |
| main | 70.86 MiB | 112.47 MiB | 190.89 MiB |

Baseline-subtracted values are secondary because the runtimes fault code pages
and native libraries differently. Total process RSS is the operational memory
cost. The increment is still useful for showing that Rust currently has a
larger single-runtime model/tokenizer representation, while main pays for each
additional tokenizer worker. From concurrency 2 to 4, main adds 78.42 MiB of
loaded RSS while Rust adds 2.25 MiB because concurrent calls share one tokenizer,
one embedding table and one compute pool.

## Findings

- Rust throughput is 61.7%, 17.7% and 27.9% higher than main at concurrency 1,
  2 and 4.
- Rust scales 32.9% from concurrency 1 to 2 and 80.1% from 2 to 4. The user
  value now controls the number of simultaneous batch jobs. Non-linear CPU
  efficiency between one and two jobs remains visible, but it is no longer
  hidden by per-batch parallelism that bypasses the option.
- An experimental implementation that parallelized all texts inside every
  batch is intentionally excluded: it made concurrency 1 use the whole machine
  and therefore violated the CLI option's task-concurrency semantics.
- Rust loaded process RSS is 9.1%, 29.1% and 49.2% lower at concurrency 1, 2
  and 4 respectively.
- Rust peak RSS under load is 58.2%, 67.5% and 71.8% lower at concurrency 1, 2
  and 4 respectively.
- The main peak is much higher than its warm resident RSS. The implementation
  converts worker `Float32Array` results into nested JavaScript number arrays;
  delayed garbage collection during sustained batches is the likely source of
  this transient peak.

These results cover one warm-cache Model2Vec workload on Apple Silicon. They do
not represent the unimplemented Rust Qwen, llama.cpp or Transformers backends,
cold download time, small batches, or long documents near the token limit.
