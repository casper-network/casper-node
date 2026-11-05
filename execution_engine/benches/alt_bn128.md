# alt_bn128 benchmarks

- Run date: **2026-10-23** (Europe/Warsaw).
- CPU: **Apple M1 Max**, 10 physical / 10 logical cores, ARM64.
- Memory: **64 GiB**.
- OS: **macOS 27.0**, build 26A428.
- Compiler: **rustc 1.98.1**, aarch64-apple-darwin.
- Build: optimized benchmark profile, LTO disabled, 16 codegen units.

Times below are **microseconds per operation**, reported as Criterion 0.5.1's
mean estimate and its 95% confidence interval. Each case uses 100 samples after
a 100 ms warmup, with a one-second measurement target. Pairing uses flat
sampling; expensive batches take longer to collect all 100 samples.

The benchmark calls the production input parser and curve operations directly.
Input generation and validation happen outside the timed region. Criterion's
estimates and sample data are saved under `target/criterion/alt_bn128_*`.

## Addition and multiplication

| Case | Lower 95% bound | Mean | Upper 95% bound |
| --- | ---: | ---: | ---: |
| Add distinct points | 2.518 | 2.526 | 2.534 |
| Add doubling | 2.494 | 2.501 | 2.508 |
| Add cancellation | 0.586 | 0.587 | 0.589 |
| Add left infinity | 0.281 | 0.282 | 0.283 |
| Add right infinity | 0.268 | 0.269 | 0.270 |
| Add both infinity | 0.054 | 0.055 | 0.055 |
| Add random corpus | 3.327 | 3.354 | 3.383 |
| Mul scalar 0 | 0.403 | 0.405 | 0.406 |
| Mul scalar 1 | 0.535 | 0.539 | 0.543 |
| Mul scalar 2 | 2.585 | 2.611 | 2.645 |
| Mul maximum u64 | 39.080 | 39.167 | 39.260 |
| Mul full-width `0x55…` | 108.099 | 108.629 | 109.191 |
| Mul full-width `0xaa…` | 112.085 | 112.631 | 113.190 |
| Mul order − 1 | 98.352 | 98.967 | 99.637 |
| Mul order | 0.416 | 0.417 | 0.419 |
| Mul order + 1 | 0.533 | 0.535 | 0.537 |
| Mul maximum U256 | 116.759 | 117.611 | 118.675 |
| Mul infinity | 34.892 | 35.109 | 35.433 |
| Mul random full-width corpus | 123.672 | 124.074 | 124.497 |

The random corpora each contain 32 deterministic inputs. The multiplication
corpus uses nonzero points and full-width scalars.

## Nonzero pairing batches

The zero-entry row measures empty input. Every entry in the other rows contains
nonzero G1 and G2 points, so the benchmark includes G2 subgroup validation,
pairing precomputation, the Miller loop, and final exponentiation.

| Pair count | Lower 95% bound | Mean | Upper 95% bound |
| ---: | ---: | ---: | ---: |
| 0 | 0.025 | 0.025 | 0.025 |
| 1 | 1,818.615 | 1,825.301 | 1,832.594 |
| 2 | 2,647.819 | 2,655.658 | 2,663.666 |
| 4 | 4,319.269 | 4,332.566 | 4,346.388 |
| 8 | 7,706.013 | 7,728.122 | 7,750.474 |
| 16 | 14,375.612 | 14,408.387 | 14,442.397 |
| 32 | 28,000.145 | 28,114.714 | 28,281.586 |
| 64 | 54,678.384 | 54,810.820 | 54,944.832 |

## Weight choice

One nonzero pair averaged about 1.83 ms, compared with
3.35 µs for addition and 124 µs for full-width multiplication over random
inputs. Pairing also gets more expensive as entries are added to a batch.

Keep the addition and multiplication input weights at **1,000 gas per byte**.
Use **800,000 gas per byte** for pairing to provide conservative headroom for
its subgroup checks and pairing work, relative to existing multiplication
pricing. The base charges remain:

| Operation | Base charge (gas) | Input weight (gas/byte) |
| --- | ---: | ---: |
| Addition | 1,200,000 | 1,000 |
| Multiplication | 3,000,000 | 1,000 |
| Pairing | 2,000,000 | 800,000 |

Each pairing entry is 192 bytes, so an entry adds **153,600,000 gas** above
the base charge.

## Run command

```sh
CARGO_PROFILE_BENCH_LTO=false CARGO_PROFILE_BENCH_CODEGEN_UNITS=16 \
  cargo bench --locked -p casper-execution-engine --bench alt_bn128 -- \
  --warm-up-time 0.1 --measurement-time 1 --sample-size 100 --noplot
```

Omit the Criterion arguments after `--` to use its default warmup and measurement
times. Append `alt_bn128/pairing` to run only the pairing group.
