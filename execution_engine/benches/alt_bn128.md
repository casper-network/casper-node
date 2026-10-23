# alt_bn128 benchmarks

- Run date: **2026-10-23** (Europe/Warsaw).
- CPU: **Apple M1 Max**, 10 physical / 10 logical cores, ARM64.
- Memory: **64 GiB**.
- OS: **macOS 27.0**, build 26A428.
- Compiler: **rustc 1.98.1**, aarch64-apple-darwin.
- Build: release optimization, LTO disabled, 16 codegen units, one test thread.

Times below are **microseconds per operation**. Percentiles are across 100
sample averages after a 100 ms warmup. Samples target 5 ms, or one operation
when it takes longer. The timed code includes the production input parser and
curve operations; input generation is outside the timed region.

## Addition and multiplication

| Case | p50 | p90 | p99 |
| --- | ---: | ---: | ---: |
| Add distinct points | 2.493 | 2.547 | 2.647 |
| Add doubling | 2.597 | 2.658 | 3.002 |
| Add cancellation | 0.604 | 0.618 | 0.697 |
| Add left infinity | 0.280 | 0.285 | 0.289 |
| Add right infinity | 0.272 | 0.276 | 0.280 |
| Add both infinity | 0.063 | 0.065 | 0.066 |
| Add random corpus | 3.198 | 3.367 | 3.538 |
| Mul scalar 0 | 0.397 | 0.405 | 0.416 |
| Mul scalar 1 | 0.524 | 0.534 | 0.549 |
| Mul scalar 2 | 2.504 | 2.557 | 2.714 |
| Mul maximum u64 | 39.333 | 40.896 | 42.826 |
| Mul full-width `0x55…` | 107.550 | 112.120 | 117.143 |
| Mul full-width `0xaa…` | 112.355 | 115.975 | 122.172 |
| Mul order − 1 | 94.009 | 98.474 | 101.339 |
| Mul order | 0.412 | 0.419 | 0.430 |
| Mul order + 1 | 0.528 | 0.539 | 0.550 |
| Mul maximum U256 | 110.236 | 112.999 | 115.380 |
| Mul infinity | 34.585 | 35.185 | 36.463 |
| Mul random full-width corpus | 128.706 | 141.011 | 165.911 |

The random corpora each contain 32 deterministic inputs. The multiplication
corpus uses nonzero points and full-width scalars.

## Nonzero pairing batches

The zero-entry row measures empty input. Every entry in the other rows contains
nonzero G1 and G2 points, so the benchmark includes G2 subgroup validation,
pairing precomputation, the Miller loop, and final exponentiation.

| Pair count | p50 | p90 | p99 |
| ---: | ---: | ---: | ---: |
| 0 | 0.025 | 0.026 | 0.027 |
| 1 | 1,841.312 | 1,915.834 | 2,174.333 |
| 2 | 2,665.333 | 2,768.208 | 2,913.167 |
| 4 | 4,330.833 | 8,655.584 | 13,287.833 |
| 8 | 7,669.917 | 7,867.125 | 8,052.417 |
| 16 | 14,331.667 | 14,564.709 | 14,641.625 |
| 32 | 27,875.416 | 28,479.792 | 29,413.375 |
| 64 | 54,608.125 | 56,573.041 | 57,558.667 |

## Weight choice

Pairing is much more expensive than addition or multiplication. One nonzero
pair took about 1.84 ms at the median, compared with 3.20 µs for addition and
129 µs for full-width multiplication over random inputs. Pairing also gets
more expensive as entries are added to a batch.

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
CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
  cargo test --locked --release -p casper-execution-engine --lib \
  runtime::cryptography::alt_bn128::benchmarks \
  -- --ignored --nocapture --test-threads=1
```
