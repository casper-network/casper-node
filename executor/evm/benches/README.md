# EVM history access benchmarks

`history_access` uses Criterion to measure EVM history access with a maintained
in-memory recent-hash cache and the LMDB height-to-hash index. Storage cases
also compare indexed hashes with full-header reads. The indexed path checks
that the header exists. All databases are temporary; the benchmark never
opens a running node's store.

## Run and filter

From the repository root:

```sh
cargo bench --locked -p casper-executor-evm --bench history_access
```

The suite registers lookup counts of 1, 256, and 8191, reader counts of 1, 4,
and 16, and sequential batch sizes of one and eight EVM calls. Select cases with
Criterion's benchmark-ID filter instead of the old custom workload flags:

```sh
# Complete history preflight.
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/preflight/complete/8191$'

# Isolate public-key, block-hash, and header decoding from database access.
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/decode/'

# Compare full-header and indexed-hash reads, 256 lookups per batch.
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/storage/.*/256$'

# Compare a populated cache with the LMDB index, including cache misses and startup.
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/(cache_storage/.*|cache_miss/.*|cache_initialize)$'

# EIP-2935 execution through the real adapter.
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/executor/eip2935_recent_spread/256$'

# Eight sequential calls, with 256 history requests per call.
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/batch/eip2935_spread_recent_8/256$'

# BLOCKHASH with repeated visits to its 256-block window.
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/batch/blockhash_spread_recent_8/8191$'

# Sixteen concurrent readers, 256 lookups per worker.
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/concurrent_readers/index_readers_16/256$'
```

The workspace's normal bench profile enables optimization and LTO. No Solidity
compiler or contract fixture build is required: the small measurement contracts
are assembled in the harness.

Criterion controls warmup, measurement time, statistical sampling, reports, and
baseline comparisons. Standard options can be used for a shorter development
measurement:

```sh
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/storage/.*/256$' \
  --warm-up-time 0.1 --measurement-time 0.2 --sample-size 10 --noplot
```

Short runs check the harness and are not production measurements. Slow cases
may require longer than the requested measurement time to collect enough
samples. The full suite can take several minutes.

For a correctness smoke run, with no statistical measurement:

```sh
cargo test --locked -p casper-executor-evm --bench history_access
```

Criterion handles Cargo's benchmark/test modes. The former `--measure`,
`--samples`, `--warmup`, `--lookups`, `--transactions`, and `--readers`
arguments are removed. Use benchmark-ID filters, `--sample-size`,
`--warm-up-time`, and `--measurement-time` instead.

## Reports and comparisons

Criterion writes reports and raw sample data under `target/criterion`, or the
directory configured through `CRITERION_HOME`. The aggregate HTML report is
normally `target/criterion/report/index.html`; each benchmark also has its own
report and estimate/sample files.

Save and compare the same cases before and after an optimization:

```sh
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/storage/.*/256$' --save-baseline before

# After changing the lookup implementation:
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/storage/.*/256$' --baseline before
```

Criterion's time estimates and confidence intervals describe time per benchmark
iteration. Samples can aggregate multiple iterations: these are not individual
request-latency observations, and their confidence bounds must not be labelled
p95 or p99. A separate per-request latency study is needed to establish those
tails. The old custom CSV/percentile reporter is no longer part of this bench.

## Workloads and units

The fixture has a real LMDB block store containing 8191 indexed synthetic V2
headers, with the EVM block number set to 8192. Initial EVM account and bytecode
records are committed to a separate LMDB global state before measurements.

Each EVM call iterates 32-byte calldata words and returns a wrapping sum of its
results. Checksums are verified against independently calculated expected header
hashes. Addition avoids the trivial cancellation of an even number of identical
hashes under XOR. The control contract returns the sum of the input heights.

The `hot` pattern repeats the latest ancestor. The deterministic `spread`
pattern visits every height in its window before repeating. BLOCKHASH retains
its 256-block window; EIP-2935 retains its 8191-block window. A count of 8191
means 8191 requests, which may revisit heights for BLOCKHASH.

| Criterion group | One iteration | Throughput elements |
| --- | --- | --- |
| `history/decode` | Decode one in-memory public key, block hash, or non-switch V2 header; compare Ed25519, Secp256k1 and System proposers | Not configured |
| `history/preflight` | Open one reader and scan until completion or a missing first, middle, or last header | Height-index/header-existence checks |
| `history/storage` | Compare decoded headers with indexed hashes, using a reader per lookup or one reader per batch. The shared-reader cases materialize the batch before checksumming | History reads |
| `history/cache_storage` | Compare recent-cache hits and indexed hashes with a reader per lookup | History reads |
| `history/cache_miss` | Compare one indexed hash lookup with a recent-cache miss that falls back to the same index | Not configured |
| `history/cache_initialize` | Warm the recent-hash cache from a stored tip and indexed headers | Not configured |
| `history/concurrent_readers` | Each worker completes one spread-index batch | History reads across all workers |
| `history/executor` | One checked EVM control, BLOCKHASH, or native EIP-2935 call | Loop iterations for control; history requests otherwise |
| `history/batch` | Full preflight followed by one or eight BLOCKHASH or EIP-2935 calls using recent hashes, shared scratch state, fresh tracking copies, and per-call effect commits | Total history requests; preflight checks are additional work |

Full-header reads obtain a cached hash populated from the storage key; they do
not recompute it. The hash-only method resolves the same persisted height index
and checks header-record existence in the same read transaction, without
copying or deserializing the header. A recent-cache hit avoids opening a reader;
on a miss the adapter opens one for the indexed lookup. Storage cases also
measure sharing one reader across a batch.

## Timing boundaries

Serial storage/preflight cases use Criterion's `iter`. Their result checks are
inside the measured routine. The individual EVM cases use `iter_batched_ref`
with `BatchSize::LargeInput`: tracking-copy forks and request cloning are setup
work. Destruction of the tracking copy and returned outcomes is outside the
timed routine. Requests are consumed by the executor inside that routine.
Execution, state application, and correctness checks remain timed.

Batch cases also use `iter_batched_ref`. Each iteration starts with a new
`ScratchGlobalState` and cloned requests, both created outside the timed routine.
The routine performs the history preflight, creates and drops a fresh
`TrackingCopy` per transaction, executes the call, and applies its accepted
effects through `ScratchGlobalState::commit_effects` before the next call.
Sequential checked nonces and a final nonce read from a new scratch view verify
that effects reach the shared state, including the last transaction's effects.
Those checks are timed; scratch state and returned outcomes are dropped after
timing ends.

The store is warmed from LMDB before the measured execution cases. Both EVM
history paths use its shared, 8192-slot cache; older or missing heights fall
back to the LMDB index. The full database-backed preflight remains timed and
runs before any execution effects, independently of the requested heights.
BLOCKHASH's 256-block window and EIP-2935's 8191-block window are unchanged.
Cache initialization is measured separately with an already warm OS page cache.
The miss fixture adds two newer headers to evict height 1 while retaining it in
LMDB; the benchmark verifies the fallback before collecting timings.

Concurrent-reader cases use `iter_custom`, which returns wall time for the
requested number of batches per worker. Thread creation and one priming read
per worker are excluded. Releasing workers, their lookups/checksum checks, and
joining them are included. Criterion normalizes this duration by its iteration
count. It measures aggregate concurrent work, not pooled worker latency
percentiles. Throughput counts `readers * lookups` per iteration.

The sequential batch now follows production's state-cache lifecycle: fresh
tracking copies share one scratch cache within a batch, and accepted effects
become visible to subsequent transactions. A new scratch state per Criterion
iteration prevents cached reads and writes from leaking between batches.
History reads bypass the scratch cache and use the block store's shared recent
hashes. Batch IDs are `{blockhash|eip2935}_{hot|spread}_recent_{transactions}`.
Individual executor IDs also include `recent`; the original storage and
concurrent-reader IDs still describe direct LMDB access. The renamed executor
and batch IDs keep these results separate from the indexed-only baseline.

The suite excludes signed transaction admission, Casper payment processing,
receipt publication, durable trie/LMDB commits, and database flushes. Scratch
effect commits are included in the batch cases. The control workload
provides execution-overhead context, but its work and gas differ from the
history contracts; subtracting it does not precisely isolate database cost.

## Maintained recent-hash cache comparison

Measurements on 2026-11-17 used an Apple M1 Max with 64 GiB RAM and Rust
1.98.1. Each case used the optimized workspace bench profile with LTO, 30
Criterion samples, a 0.5-second warmup, and a 2-second requested measurement
window, extended where needed. The indexed-only batch baseline was measured
before the cache change; the cache batch was measured afterward on the same
machine with the same workload and timing boundaries. These are separate runs,
not a simultaneous comparison or production latency percentiles.

Times are Criterion point estimates for the entire spread-pattern batch,
including one 8191-header preflight, execution, and scratch effect commits:

| Route | Transactions | Requests per transaction | Indexed only | Recent cache | Indexed / cache |
| --- | ---: | ---: | ---: | ---: | ---: |
| BLOCKHASH | 8 | 1 | 4.179 ms | 4.196 ms | 1.00x |
| BLOCKHASH | 8 | 256 | 7.387 ms | 4.731 ms | 1.56x |
| BLOCKHASH | 1 | 8191 | 15.769 ms | 5.077 ms | 3.11x |
| BLOCKHASH | 8 | 8191 | 98.474 ms | 11.368 ms | 8.66x |
| EIP-2935 | 8 | 1 | 4.295 ms | 4.411 ms | 0.97x |
| EIP-2935 | 8 | 256 | 8.175 ms | 5.342 ms | 1.53x |
| EIP-2935 | 1 | 8191 | 22.164 ms | 7.421 ms | 2.99x |
| EIP-2935 | 8 | 8191 | 124.320 ms | 30.381 ms | 4.09x |

The confidence intervals for the one-request cases overlap; the table does not
establish a change there. A separate complete preflight measurement was 3.911
ms, explaining why small batches see little benefit. EIP-2935 still has native
call and EVM work beyond the history lookup.

For direct storage comparisons measured in the same binary, 8191 spread reads
took 11.621 ms through the LMDB index and 82.860 us through the warmed cache
(about 140x). Those figures include the checksum loop but exclude preflight
and EVM execution; they are not the batch speedup. A cached lookup that missed
and fell back to LMDB took 1.189 us versus 1.154 us for an indexed lookup, with
overlapping confidence intervals. Cache initialization from 8191 stored headers
took 3.918 ms with warm OS pages. The fixture does not simulate cold startup,
concurrent writers, or a production-sized database.

To repeat the current measurements:

```sh
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^(history/batch/(blockhash|eip2935)_spread_recent_(8/(1|256|8191)|1/8191)|history/cache_storage/(index|recent)_(hot|spread)/(256|8191)|history/cache_miss/.*|history/cache_initialize|history/preflight/complete/8191)$' \
  --warm-up-time 0.5 --measurement-time 2 --sample-size 30 --noplot
```

The indexed-only batch baseline was measured from revision `10df91f1c9` using
the same settings and `history/batch/*_spread_index_*` IDs. In the original
working copy, raw Criterion reports are in `target/criterion` and the commands,
source snapshots, and logs are in `target/evm-history/recent-hashes-comparison/`.

## Historical LMDB index comparison

The comparison retains fresh measurements of the previous full-header and
block-cache adapters, then measures the current hash-only adapter using the
same fixtures and Criterion settings. The storage microbenchmarks additionally
compare full-header and indexed-hash reads in the same binary. That older
cache was a per-block lazy prototype, distinct from the maintained cache in
the current benchmark.

Measurements on 2026-11-13 used an Apple M1 Max with 64 GiB RAM and the
optimized workspace bench profile with LTO. Each case collected 30 Criterion
samples after 0.5 seconds of warmup, with a requested 2-second measurement
window extended as needed. The adapter comparison uses sequential before/after
binaries on the same machine; the raw storage comparison runs both methods in
the same binary. These are warm working-set measurements on development hardware.

Times are Criterion point estimates per complete spread-pattern batch, with
the full preflight, fresh tracking copies and accepted scratch effects included:

| Route | Transactions | Requests per transaction | Full header | Former cache | Indexed hash |
| --- | ---: | ---: | ---: | ---: | ---: |
| BLOCKHASH | 1 | 8191 | 53.29 ms | 8.26 ms | 17.65 ms |
| BLOCKHASH | 8 | 256 | 17.21 ms | 6.61 ms | 8.25 ms |
| BLOCKHASH | 8 | 8191 | 435.06 ms | 13.42 ms | 104.07 ms |
| EIP-2935 | 1 | 8191 | 68.18 ms | 67.54 ms | 23.51 ms |
| EIP-2935 | 8 | 256 | 20.76 ms | 9.79 ms | 9.34 ms |
| EIP-2935 | 8 | 8191 | 447.89 ms | 89.99 ms | 151.19 ms |

The indexed adapter is about 3–4x faster than full-header access for large
batches. The former cache still wins on heavy reuse: in the eight-call,
8191-request cases, the indexed path takes about 7.8x as long for BLOCKHASH and
1.7x as long for EIP-2935. A single EIP-2935 call visiting 8191 distinct heights
benefits from skipping header decoding on every lookup; the former cache
provided no reuse in that workload. The cache and index confidence intervals
overlap for eight EIP-2935 calls with 256 requests, so their small difference
does not establish an improvement. Small workloads remain dominated by preflight.

Raw storage results, with a reader per lookup and no execution or preflight:

| Lookups | Full header | Indexed hash | Speedup |
| ---: | ---: | ---: | ---: |
| 256 | 1.435 ms | 0.393 ms | 3.65x |
| 8191 | 47.076 ms | 12.540 ms | 3.75x |

The original batch IDs in this command apply to the earlier indexed-only
revision; the raw storage cases are still present:

```sh
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^history/(batch/.*_spread_index_|storage/(header|index)_spread_per_lookup_reader/(256|8191)$)' \
  --warm-up-time 0.5 --measurement-time 2 --sample-size 30 \
  --save-baseline lmdb-index-comparison-after --noplot
```

The original working copy retains sources, commands, samples, confidence
intervals and logs under `target/evm-history/lmdb-index-comparison/`. Previous
cache source snapshots remain available there for reproducing that historical
variant; the current benchmark no longer registers its IDs. `results.json`
contains the complete three-way comparison, including confidence intervals.

## Why header decoding is expensive

Header decoding reconstructs the proposer public key. `BlockHeaderV2::from_bytes`
calls `PublicKey::from_bytes`, which constructs an Ed25519 or Secp256k1 verifying
key. For Ed25519, `ed25519-dalek` decompresses the encoded Edwards curve point,
including finite-field square-root arithmetic. This work repeats on every
header read even when all LMDB pages are resident in RAM. It is public-key
decoding and validation; no block signature verification is performed here.

The `history/decode` cases isolate that work using in-memory serialized inputs.
Serialization and roundtrip checks are setup work. Each timed iteration decodes
the input, checks that no bytes remain, and consumes/drops the decoded result.
Headers have identical ordinary fields and no era-end data; only the proposer
differs. `System` is a diagnostic control without an encoded curve point, not
a representative validator proposer. These independent component measurements
are explanatory rather than an exact additive CPU profile.

A follow-up run on the same M1 Max used 50 Criterion samples per case, 0.5-second
warmup, and a 2-second requested measurement window:

| In-memory decode | Time per value |
| --- | ---: |
| 32-byte block hash | 2.70 ns |
| Ed25519 public key | 3.85 us |
| Header with Ed25519 proposer | 4.78 us |
| Secp256k1 public key | 5.20 us |
| Header with Secp256k1 proposer | 5.33 us |
| Header with System proposer | 65.69 ns |

The same run measured 8191 full-header lookups at 45.522 ms (5.558 us per
lookup), versus 12.195 ms (1.489 us per lookup) for indexed hashes. Both methods
open one reader per lookup, resolve the height index, and access the header
record. The difference is decoding the complete value versus checking its
existence. The saved 4.069 us per lookup adds up to 33.328 ms per batch; the
public-key decode measurement explains most of that gap.

The block store already installs the hash from the storage key into a decoded
header's hash cache, so the full-header path does not recompute the block hash.
Both paths also serialize the same header hash key. The main avoided work in
these non-switch fixtures is reconstructing the proposer key, rather than
hashing a header or parsing large era-end collections. Exact ratios depend on
proposer type, header contents, hardware and memory residency.

Reproduce with:

```sh
cargo bench --locked -p casper-executor-evm --bench history_access -- \
  '^(history/decode/.*|history/storage/(header|index)_spread_per_lookup_reader/8191)$' \
  --warm-up-time 0.5 --measurement-time 2 --sample-size 50 \
  --save-baseline history-decode-breakdown --noplot
```

The original working copy retains samples, confidence intervals, sources and
logs under `target/evm-history/history-decode-breakdown/`.

## Environment and scope

Record the source revision and worktree changes, Rust version, CPU model,
memory, OS, filesystem/device, temporary-directory location, power mode, build
flags, and competing workload with every measurement. For example:

```sh
git rev-parse HEAD
git status --short
rustc --version
uname -a
```

Use `TMPDIR` to place temporary stores on the device being evaluated. Keep
power and background workload consistent and repeat runs to assess variation.

These are warm working-set measurements. Setup and warmup populate the OS page
cache, and 8191 headers form a small database. Reopening a reader is not a cold
disk read. The headers are non-switch V2 headers, so large era-end metadata and
legacy headers are not represented. Neither are a production-sized database,
concurrent writers, synchronization load, or reader-slot exhaustion.

Repeat on minimum validator hardware, with those conditions represented, before
using the results to choose production gas costs or lane limits.

## Historical measurements

The development measurements from 2026-11-13 used the previous fixed-iteration
CSV/percentile harness. Their raw results, environment details, and original
source snapshots remain in `target/evm-history/history-access-measurements.zip`
in the original working copy. They are historical data, not Criterion
baselines, and their p99 values are not directly comparable to Criterion's time
estimates or confidence intervals.
