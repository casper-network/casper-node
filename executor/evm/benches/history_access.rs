//! Criterion benchmarks for EVM history access. See README.md for timing boundaries.

use std::{
    hint::black_box,
    sync::Barrier,
    thread,
    time::{Duration, Instant},
};

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};

use casper_executor_evm::{
    BlockContext, CallRequest, CallValidation, EvmExecutor, ExecuteKind, ExecuteRequest,
    ExecutionStatus, BLOCK_HASH_HISTORY,
};
use casper_storage::{
    block_store::lmdb::LmdbBlockStore,
    data_access_layer::DataAccessLayer,
    eip2935,
    global_state::state::{
        lmdb::{make_temporary_global_state, LmdbGlobalState},
        CommitProvider, ScratchProvider, StateProvider, StateReader,
    },
    TrackingCopy,
};
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    evm, BlockHash, BlockHeader, BlockHeaderV2, ByteCode, ByteCodeKind, CLValue, Digest, EraId,
    EvmAddr, EvmConfig, Key, ProtocolVersion, PublicKey, SecretKey, StoredValue, Timestamp,
    EVM_TRANSACTION_GAS_LIMIT, U256, U512,
};
use once_cell::sync::OnceCell;
use revm::bytecode::opcode;

const HEIGHT: u64 = eip2935::HISTORY_BUFFER_LENGTH + 1;
const BLOCK_GAS: u64 = 30_000_000;
const SENDER: evm::Address = evm::Address::new([0x31; 20]);
const LOOKUP_COUNTS: [usize; 3] = [1, 256, eip2935::HISTORY_BUFFER_LENGTH as usize];
const READER_COUNTS: [usize; 3] = [1, 4, 16];
const TRANSACTION_COUNTS: [usize; 2] = [1, 8];

fn header(height: u64, parent: BlockHash, proposer: PublicKey) -> BlockHeader {
    BlockHeader::V2(BlockHeaderV2::new(
        parent,
        Digest::hash("history benchmark state"),
        Digest::hash(height.to_le_bytes()),
        false,
        Digest::hash("history benchmark seed"),
        None,
        Timestamp::from(1_714_000_000 + height * 1_000),
        EraId::new(1),
        height,
        ProtocolVersion::V2_0_0,
        proposer,
        1,
        None,
        OnceCell::new(),
    ))
}

fn headers() -> Vec<BlockHeader> {
    let key = SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&key);
    let mut parent = BlockHash::new(Digest::hash("history benchmark parent"));
    (1..HEIGHT)
        .map(|height| {
            let header = header(height, parent, proposer.clone());
            parent = header.block_hash();
            header
        })
        .collect()
}

fn decode<T: FromBytes>(bytes: &[u8]) -> T {
    let (value, remainder) = T::from_bytes(black_box(bytes)).unwrap();
    assert!(remainder.is_empty());
    black_box(value)
}

fn bench_decode(c: &mut Criterion, headers: &[BlockHeader]) {
    let ed25519_key =
        PublicKey::from(&SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap());
    let secp256k1_key = PublicKey::from(&SecretKey::secp256k1_from_bytes([43; 32]).unwrap());
    let ed25519_bytes = ed25519_key.to_bytes().unwrap();
    let secp256k1_bytes = secp256k1_key.to_bytes().unwrap();
    let hash = headers[0].block_hash();
    let hash_bytes = hash.to_bytes().unwrap();
    let header_bytes = headers[0].to_bytes().unwrap();
    let system_header = header(1, *headers[0].parent_hash(), PublicKey::System);
    let system_bytes = system_header.to_bytes().unwrap();
    let secp256k1_header = header(1, *headers[0].parent_hash(), secp256k1_key.clone());
    let secp256k1_header_bytes = secp256k1_header.to_bytes().unwrap();

    assert_eq!(decode::<PublicKey>(&ed25519_bytes), ed25519_key);
    assert_eq!(decode::<PublicKey>(&secp256k1_bytes), secp256k1_key);
    assert_eq!(decode::<BlockHash>(&hash_bytes), hash);
    assert_eq!(decode::<BlockHeader>(&header_bytes), headers[0]);
    assert_eq!(decode::<BlockHeader>(&system_bytes), system_header);
    assert_eq!(
        decode::<BlockHeader>(&secp256k1_header_bytes),
        secp256k1_header
    );

    let mut group = c.benchmark_group("history/decode");
    group.bench_function("public_key_ed25519", |b| {
        b.iter(|| decode::<PublicKey>(&ed25519_bytes));
    });
    group.bench_function("public_key_secp256k1", |b| {
        b.iter(|| decode::<PublicKey>(&secp256k1_bytes));
    });
    group.bench_function("block_hash", |b| {
        b.iter(|| decode::<BlockHash>(&hash_bytes));
    });
    group.bench_function("header_ed25519", |b| {
        b.iter(|| decode::<BlockHeader>(&header_bytes));
    });
    group.bench_function("header_secp256k1", |b| {
        b.iter(|| decode::<BlockHeader>(&secp256k1_header_bytes));
    });
    group.bench_function("header_system", |b| {
        b.iter(|| decode::<BlockHeader>(&system_bytes));
    });
    group.finish();
}

fn store(headers: &[BlockHeader], missing_height: Option<u64>) -> LmdbBlockStore {
    let mut store = LmdbBlockStore::new_temporary(64 * 1024 * 1024).unwrap();
    if let Some(height) = missing_height {
        let headers: Vec<_> = headers
            .iter()
            .filter(|header| header.height() != height)
            .cloned()
            .collect();
        store.put_block_headers(&headers).unwrap();
    } else {
        store.put_block_headers(headers).unwrap();
    }
    store
}

fn heights(count: usize, window: u64, spread: bool) -> Vec<u64> {
    (0..count)
        .map(|index| {
            if spread {
                // Coprime to both window lengths; deterministic and covers each window.
                HEIGHT - window + (index as u64 * 73) % window
            } else {
                HEIGHT - 1
            }
        })
        .collect()
}

fn checksum(headers: &[BlockHeader], heights: &[u64]) -> U256 {
    sum_block_hashes(
        heights
            .iter()
            .map(|&height| headers[height as usize - 1].block_hash()),
    )
}

fn sum_block_hashes(hashes: impl IntoIterator<Item = BlockHash>) -> U256 {
    hashes.into_iter().fold(U256::zero(), |sum, hash| {
        sum.overflowing_add(U256::from_big_endian(hash.as_ref())).0
    })
}

#[derive(Clone, Copy)]
enum HistoryLookup {
    Header,
    Index,
}

fn read_batch(
    store: &LmdbBlockStore,
    heights: &[u64],
    shared_reader: bool,
    lookup: HistoryLookup,
) -> U256 {
    match (lookup, shared_reader) {
        (HistoryLookup::Header, true) => sum_block_hashes(
            store
                .read_block_headers_at_heights(heights)
                .unwrap()
                .into_iter()
                .map(|header| header.unwrap().block_hash()),
        ),
        (HistoryLookup::Index, true) => sum_block_hashes(
            store
                .read_block_hashes_at_heights(heights)
                .unwrap()
                .into_iter()
                .map(Option::unwrap),
        ),
        (HistoryLookup::Header, false) => sum_block_hashes(heights.iter().map(|&height| {
            store
                .read_block_header_at_height(height)
                .unwrap()
                .unwrap()
                .block_hash()
        })),
        (HistoryLookup::Index, false) => sum_block_hashes(
            heights
                .iter()
                .map(|&height| store.read_block_hash_at_height(height).unwrap().unwrap()),
        ),
    }
}

fn read_recent_batch(store: &LmdbBlockStore, heights: &[u64]) -> U256 {
    sum_block_hashes(
        heights
            .iter()
            .map(|&height| store.read_recent_block_hash(height).unwrap().unwrap()),
    )
}

// One Criterion iteration means one storage batch per worker. Only the aggregate
// wall time is returned; this does not measure individual worker latency percentiles.
fn read_parallel(
    store: &LmdbBlockStore,
    heights: &[u64],
    expected: U256,
    readers: usize,
    iterations: u64,
) -> Duration {
    let barrier = Barrier::new(readers + 1);
    thread::scope(|scope| {
        let handles: Vec<_> = (0..readers)
            .map(|_| {
                let barrier = &barrier;
                scope.spawn(move || {
                    // Prime each worker's reader before releasing the timed workload.
                    assert_eq!(
                        read_batch(store, heights, false, HistoryLookup::Index),
                        expected
                    );
                    barrier.wait();
                    barrier.wait();
                    for _ in 0..iterations {
                        assert_eq!(
                            black_box(read_batch(store, heights, false, HistoryLookup::Index)),
                            expected
                        );
                    }
                })
            })
            .collect();
        barrier.wait();
        let start = Instant::now();
        barrier.wait();
        for handle in handles {
            handle.join().unwrap();
        }
        start.elapsed()
    })
}

#[derive(Clone, Copy)]
enum Route {
    Control,
    Blockhash,
    Eip2935,
}

impl Route {
    fn address(self) -> evm::Address {
        evm::Address::new(
            [match self {
                Self::Control => 0x41,
                Self::Blockhash => 0x42,
                Self::Eip2935 => 0x43,
            }; 20],
        )
    }

    fn runtime(self) -> Vec<u8> {
        // Iterate 32-byte calldata words and return the wrapping sum of all results.
        // Constant-sized bytecode allows 8191 lookups without oversized contracts.
        let mut code = vec![opcode::PUSH0]; // calldata offset
        let loop_pc = code.len() as u16;
        code.extend([
            opcode::JUMPDEST,
            opcode::DUP1,
            opcode::CALLDATASIZE,
            opcode::EQ,
            opcode::PUSH2,
        ]);
        let end_patch = code.len();
        code.extend([0, 0, opcode::JUMPI, opcode::DUP1, opcode::CALLDATALOAD]);
        let mut failure_patch = None;
        match self {
            Self::Control => {} // Sum the heights, without accessing history.
            Self::Blockhash => code.push(opcode::BLOCKHASH),
            Self::Eip2935 => {
                code.extend([
                    opcode::PUSH1,
                    32,
                    opcode::MSTORE, // request at memory[32..64]
                    opcode::PUSH1,
                    32, // return size
                    opcode::PUSH1,
                    64, // return offset
                    opcode::PUSH1,
                    32, // argument size
                    opcode::PUSH1,
                    32, // argument offset
                    opcode::PUSH20,
                ]);
                code.extend(eip2935::BLOCK_HASH_HISTORY_ADDRESS.as_ref());
                code.extend([
                    opcode::GAS,
                    opcode::STATICCALL,
                    opcode::ISZERO,
                    opcode::PUSH2,
                ]);
                failure_patch = Some(code.len());
                code.extend([0, 0, opcode::JUMPI, opcode::PUSH1, 64, opcode::MLOAD]);
            }
        }
        code.extend([
            opcode::PUSH0,
            opcode::MLOAD,
            opcode::ADD,
            opcode::PUSH0,
            opcode::MSTORE,
            opcode::PUSH1,
            32,
            opcode::ADD,
            opcode::PUSH2,
        ]);
        code.extend(loop_pc.to_be_bytes());
        code.push(opcode::JUMP);
        let end_pc = code.len() as u16;
        code[end_patch..end_patch + 2].copy_from_slice(&end_pc.to_be_bytes());
        code.extend([
            opcode::JUMPDEST,
            opcode::POP,
            opcode::PUSH1,
            32,
            opcode::PUSH0,
            opcode::RETURN,
        ]);
        if let Some(patch) = failure_patch {
            let failure_pc = code.len() as u16;
            code[patch..patch + 2].copy_from_slice(&failure_pc.to_be_bytes());
            code.extend([
                opcode::JUMPDEST,
                opcode::PUSH0,
                opcode::PUSH0,
                opcode::REVERT,
            ]);
        }
        code
    }
}

fn account(address: evm::Address, balance: u64, code: Vec<u8>) -> Vec<(Key, StoredValue)> {
    let purse = evm::deterministic_purse(address);
    let hash = evm::Hash::new(alloy_primitives::keccak256(&code).0);
    vec![
        (
            Key::Evm(EvmAddr::Account(address)),
            StoredValue::CLValue(CLValue::from_t(Key::URef(purse)).unwrap()),
        ),
        (
            Key::Evm(EvmAddr::Nonce(address)),
            StoredValue::CLValue(CLValue::from_t(0u64).unwrap()),
        ),
        (
            Key::Evm(EvmAddr::CodeHash(address)),
            StoredValue::CLValue(CLValue::from_t(hash).unwrap()),
        ),
        (
            Key::Balance(purse.addr()),
            StoredValue::CLValue(CLValue::from_t(U512::from(balance)).unwrap()),
        ),
        (
            Key::Evm(EvmAddr::ByteCode(hash)),
            StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmOsaka, code)),
        ),
    ]
}

fn request(route: Route, heights: &[u64], nonce: u64, gas_limit: u64) -> ExecuteRequest {
    let input = heights
        .iter()
        .flat_map(|height| {
            let mut word = [0; 32];
            word[24..].copy_from_slice(&height.to_be_bytes());
            word
        })
        .collect();
    ExecuteRequest {
        block: BlockContext {
            number: HEIGHT,
            timestamp: 1_714_010_000,
            beneficiary: evm::Address::ZERO,
            gas_limit: None,
            base_fee: None,
            prevrandao: evm::Hash::ZERO,
        },
        kind: ExecuteKind::Call(CallRequest {
            from: SENDER,
            to: Some(route.address()),
            value: U256::zero(),
            input,
            gas_limit,
            gas_price: 0,
            nonce,
            validation: CallValidation::Checked,
        }),
    }
}

fn bench_preflight(c: &mut Criterion, headers: &[BlockHeader], full_store: &LmdbBlockStore) {
    let mut group = c.benchmark_group("history/preflight");
    for (name, missing) in [
        ("complete", None),
        ("missing_first", Some(1)),
        ("missing_middle", Some(HEIGHT / 2)),
        ("missing_last", Some(HEIGHT - 1)),
    ] {
        let incomplete = missing.map(|height| store(headers, Some(height)));
        let store = incomplete.as_ref().unwrap_or(full_store);
        let checks = missing.unwrap_or(HEIGHT - 1);
        group.throughput(Throughput::Elements(checks));
        group.bench_function(BenchmarkId::new(name, checks), |b| {
            b.iter(|| {
                let result = store.first_missing_block_header_height(1..HEIGHT).unwrap();
                assert_eq!(result, missing);
                black_box(result)
            });
        });
    }
    group.finish();
}

fn bench_storage(c: &mut Criterion, headers: &[BlockHeader], store: &LmdbBlockStore) {
    let mut group = c.benchmark_group("history/storage");
    for count in LOOKUP_COUNTS {
        group.throughput(Throughput::Elements(count as u64));
        for spread in [false, true] {
            let heights = heights(count, eip2935::HISTORY_BUFFER_LENGTH, spread);
            let expected = checksum(headers, &heights);
            for shared in [false, true] {
                for lookup in [HistoryLookup::Header, HistoryLookup::Index] {
                    let method = match lookup {
                        HistoryLookup::Header => "header",
                        HistoryLookup::Index => "index",
                    };
                    let name = format!(
                        "{}_{}_{}",
                        method,
                        if spread { "spread" } else { "hot" },
                        if shared {
                            "shared_reader"
                        } else {
                            "per_lookup_reader"
                        },
                    );
                    group.bench_function(BenchmarkId::new(name, count), |b| {
                        b.iter(|| {
                            let actual = black_box(read_batch(store, &heights, shared, lookup));
                            assert_eq!(actual, expected);
                            actual
                        });
                    });
                }
            }
        }
    }
    group.finish();
}

fn bench_recent_history(c: &mut Criterion, headers: &[BlockHeader], store: &LmdbBlockStore) {
    let mut group = c.benchmark_group("history/cache_storage");
    for count in LOOKUP_COUNTS {
        group.throughput(Throughput::Elements(count as u64));
        for spread in [false, true] {
            let heights = heights(count, eip2935::HISTORY_BUFFER_LENGTH, spread);
            let expected = checksum(headers, &heights);
            let pattern = if spread { "spread" } else { "hot" };
            for cached in [false, true] {
                let method = if cached { "recent" } else { "index" };
                group.bench_function(
                    BenchmarkId::new(format!("{method}_{pattern}"), count),
                    |b| {
                        b.iter(|| {
                            let actual = if cached {
                                read_recent_batch(store, &heights)
                            } else {
                                read_batch(store, &heights, false, HistoryLookup::Index)
                            };
                            assert_eq!(black_box(actual), expected);
                            actual
                        });
                    },
                );
            }
        }
    }
    group.finish();

    c.bench_function("history/cache_initialize", |b| {
        b.iter(|| store.initialize_recent_history().unwrap());
    });

    // The next two consecutive blocks evict height 1, which remains in LMDB.
    let mut missed_store = self::store(headers, None);
    let key = SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&key);
    let next = header(
        HEIGHT,
        headers.last().unwrap().block_hash(),
        proposer.clone(),
    );
    let tip = header(HEIGHT + 1, next.block_hash(), proposer);
    missed_store.put_block_headers(&[next, tip]).unwrap();
    let expected = checksum(headers, &[1]);
    assert_eq!(read_recent_batch(&missed_store, &[1]), expected);
    let mut group = c.benchmark_group("history/cache_miss");
    for cached in [false, true] {
        let method = if cached { "recent" } else { "index" };
        group.bench_function(method, |b| {
            b.iter(|| {
                let actual = if cached {
                    read_recent_batch(&missed_store, &[1])
                } else {
                    read_batch(&missed_store, &[1], false, HistoryLookup::Index)
                };
                assert_eq!(black_box(actual), expected);
                actual
            });
        });
    }
    group.finish();
}

fn bench_concurrent_readers(c: &mut Criterion, headers: &[BlockHeader], store: &LmdbBlockStore) {
    let mut group = c.benchmark_group("history/concurrent_readers");
    for count in LOOKUP_COUNTS {
        let heights = heights(count, eip2935::HISTORY_BUFFER_LENGTH, true);
        let expected = checksum(headers, &heights);
        for readers in READER_COUNTS {
            group.throughput(Throughput::Elements((count * readers) as u64));
            group.bench_function(
                BenchmarkId::new(format!("index_readers_{readers}"), count),
                |b| {
                    b.iter_custom(|iterations| {
                        read_parallel(store, &heights, expected, readers, iterations)
                    })
                },
            );
        }
    }
    group.finish();
}

fn bench_executor(c: &mut Criterion, headers: &[BlockHeader], block_store: LmdbBlockStore) {
    let routes = [Route::Control, Route::Blockhash, Route::Eip2935];
    let mut initial_state = account(SENDER, 1_000_000, Vec::new());
    for route in routes {
        initial_state.extend(account(route.address(), 0, route.runtime()));
    }
    initial_state.extend(account(
        eip2935::BLOCK_HASH_HISTORY_ADDRESS,
        0,
        eip2935::BLOCK_HASH_HISTORY_CODE.to_vec(),
    ));
    let (global_state, root, _directory) = make_temporary_global_state(initial_state);
    let reader = global_state.checkout(root).unwrap().unwrap();
    let base = TrackingCopy::new(reader, 5, false);
    let data = DataAccessLayer {
        block_store,
        state: global_state,
        max_query_depth: 5,
        enable_addressable_entity: false,
    };
    let executor = EvmExecutor::new(EvmConfig {
        enabled: true,
        chain_id: 7,
        block_gas_limit: BLOCK_GAS,
        ..EvmConfig::default()
    });

    let mut group = c.benchmark_group("history/executor");
    for count in LOOKUP_COUNTS {
        group.throughput(Throughput::Elements(count as u64));
        for route in routes {
            for spread in [false, true] {
                let window = match route {
                    Route::Blockhash => BLOCK_HASH_HISTORY,
                    _ => eip2935::HISTORY_BUFFER_LENGTH,
                };
                let heights = heights(count, window, spread);
                let expected = match route {
                    Route::Control => heights
                        .iter()
                        .fold(U256::zero(), |sum, height| sum + U256::from(*height)),
                    _ => checksum(headers, &heights),
                };
                let mut expected_bytes = [0; 32];
                expected.to_big_endian(&mut expected_bytes);
                let route_name = match route {
                    Route::Control => "control",
                    Route::Blockhash => "blockhash",
                    Route::Eip2935 => "eip2935",
                };
                let name = format!(
                    "{route_name}_recent_{}",
                    if spread { "spread" } else { "hot" },
                );
                let request = request(route, &heights, 0, EVM_TRANSACTION_GAS_LIMIT);
                group.bench_function(BenchmarkId::new(name, count), |b| {
                    b.iter_batched_ref(
                        || (base.fork2(), Some(request.clone())),
                        |(state, request)| {
                            let outcome = executor
                                .execute(&data, state, request.take().unwrap())
                                .unwrap();
                            assert_eq!(outcome.status, ExecutionStatus::Success);
                            assert_eq!(outcome.output, expected_bytes);
                            black_box(outcome)
                        },
                        BatchSize::LargeInput,
                    );
                });
            }
        }
    }
    group.finish();

    bench_batches(c, headers, &data, &executor, root);
}

fn bench_batches(
    c: &mut Criterion,
    headers: &[BlockHeader],
    data: &DataAccessLayer<LmdbGlobalState>,
    executor: &EvmExecutor,
    root: Digest,
) {
    let mut group = c.benchmark_group("history/batch");
    for route in [Route::Blockhash, Route::Eip2935] {
        let (route_name, window) = match route {
            Route::Blockhash => ("blockhash", BLOCK_HASH_HISTORY),
            Route::Eip2935 => ("eip2935", eip2935::HISTORY_BUFFER_LENGTH),
            Route::Control => unreachable!(),
        };
        for spread in [false, true] {
            let pattern = if spread { "spread" } else { "hot" };
            for count in LOOKUP_COUNTS {
                let heights = heights(count, window, spread);
                let expected = checksum(headers, &heights);
                let mut expected_bytes = [0; 32];
                expected.to_big_endian(&mut expected_bytes);
                for transactions in TRANSACTION_COUNTS {
                    let requests: Vec<_> = (0..transactions)
                        .map(|nonce| {
                            request(
                                route,
                                &heights,
                                nonce as u64,
                                EVM_TRANSACTION_GAS_LIMIT.min(BLOCK_GAS / transactions as u64),
                            )
                        })
                        .collect();
                    let expected_nonce =
                        StoredValue::CLValue(CLValue::from_t(transactions as u64).unwrap());
                    group.throughput(Throughput::Elements((count * transactions) as u64));
                    group.bench_function(
                        BenchmarkId::new(
                            format!("{route_name}_{pattern}_recent_{transactions}"),
                            count,
                        ),
                        |b| {
                            b.iter_batched_ref(
                                || (data.get_scratch_global_state(), requests.clone()),
                                |(scratch, requests)| {
                                    let missing = data
                                        .block_store
                                        .first_missing_block_header_height(1..HEIGHT)
                                        .unwrap();
                                    assert_eq!(missing, None);
                                    let mut state_root_hash = root;
                                    let outcomes: Vec<_> = std::mem::take(requests)
                                        .into_iter()
                                        .map(|request| {
                                            let mut tracking_copy = scratch
                                                .tracking_copy(state_root_hash)
                                                .unwrap()
                                                .unwrap();
                                            let outcome = executor
                                                .execute(data, &mut tracking_copy, request)
                                                .unwrap();
                                            assert_eq!(outcome.status, ExecutionStatus::Success);
                                            assert_eq!(outcome.output, expected_bytes);
                                            state_root_hash = scratch
                                                .commit_effects(
                                                    state_root_hash,
                                                    tracking_copy.effects(),
                                                )
                                                .unwrap();
                                            outcome
                                        })
                                        .collect();
                                    let committed =
                                        scratch.checkout(state_root_hash).unwrap().unwrap();
                                    assert_eq!(
                                        committed
                                            .read(&Key::Evm(EvmAddr::Nonce(SENDER)))
                                            .unwrap()
                                            .as_ref(),
                                        Some(&expected_nonce)
                                    );
                                    black_box(outcomes)
                                },
                                BatchSize::LargeInput,
                            );
                        },
                    );
                }
            }
        }
    }
    group.finish();
}

fn history_access(c: &mut Criterion) {
    let headers = headers();
    let store = store(&headers, None);
    bench_decode(c, &headers);
    bench_preflight(c, &headers, &store);
    bench_storage(c, &headers, &store);
    bench_recent_history(c, &headers, &store);
    bench_concurrent_readers(c, &headers, &store);
    bench_executor(c, &headers, store);
}

criterion_group!(benches, history_access);
criterion_main!(benches);
