//! Unit tests for the storage component.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    convert::TryInto,
    fs::{self, File},
    io,
    iter::{self, FromIterator},
    path::{Path, PathBuf},
    sync::Arc,
};

use lmdb::Transaction as LmdbTransaction;
use once_cell::sync::Lazy;
use rand::{prelude::SliceRandom, Rng};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use casper_types::{
    execution::{
        execution_result_v1::{ExecutionEffect, ExecutionResultV1, Transform, TransformEntry},
        ExecutionResult, ExecutionResultV2,
    },
    generate_ed25519_keypair,
    system::auction::UnbondingPurse,
    testing::TestRng,
    AccessRights, AvailableBlockRange, Block, BlockHash, BlockHeader, BlockSignatures, BlockV2,
    Chainspec, ChainspecRawBytes, Deploy, DeployApprovalsHash, DeployHash, Digest, EraId,
    ExecutionInfo, FinalitySignature, Key, ProtocolVersion, PublicKey, SecretKey,
    SignedBlockHeader, TestBlockBuilder, TestBlockV1Builder, TimeDiff, Transaction,
    TransactionApprovalsHash, TransactionHash, TransactionV1Hash, Transfer, URef, U512,
};
use tempfile::tempdir;

use super::{
    initialize_block_metadata_db,
    lmdb_ext::{deserialize_internal, serialize_internal, TransactionExt, WriteTransactionExt},
    move_storage_files_to_network_subdir, should_move_storage_files_to_network_subdir,
    BlockHashHeightAndEra, Config, Storage, FORCE_RESYNC_FILE_NAME,
};
use crate::{
    components::fetcher::{FetchItem, FetchResponse},
    effect::{
        requests::{MarkBlockCompletedRequest, StorageRequest},
        Multiple,
    },
    testing::{ComponentHarness, UnitTestEvent},
    types::{
        sync_leap_validation_metadata::SyncLeapValidationMetaData, ApprovalsHashes, LegacyDeploy,
        SyncLeapIdentifier, TransactionWithFinalizedApprovals,
    },
    utils::{Loadable, WithDir},
};

const RECENT_ERA_COUNT: u64 = 7;
const MAX_TTL: TimeDiff = TimeDiff::from_seconds(86400);

fn new_config(harness: &ComponentHarness<UnitTestEvent>) -> Config {
    const MIB: usize = 1024 * 1024;

    // Restrict all stores to 50 mibibytes, to catch issues before filling up the entire disk.
    Config {
        path: harness.tmp.path().join("storage"),
        max_block_store_size: 50 * MIB,
        max_deploy_store_size: 50 * MIB,
        max_deploy_metadata_store_size: 50 * MIB,
        max_state_store_size: 50 * MIB,
        enable_mem_deduplication: true,
        mem_pool_prune_interval: 4,
    }
}

fn block_headers_into_heights(block_headers: &[BlockHeader]) -> Vec<u64> {
    block_headers
        .iter()
        .map(|block_header| block_header.height())
        .collect()
}

fn signed_block_headers_into_heights(signed_block_headers: &[SignedBlockHeader]) -> Vec<u64> {
    signed_block_headers
        .iter()
        .map(|signed_block_header| signed_block_header.block_header().height())
        .collect()
}

fn create_sync_leap_test_chain(
    non_signed_blocks: &[u64], // indices of blocks to not be signed
    include_switch_block_at_tip: bool,
    maybe_recent_era_count: Option<u64>, // if Some, override default `RECENT_ERA_COUNT`
) -> (Storage, Chainspec, Vec<Block>) {
    // Test chain:
    //      S0      S1 B2 B3 S4 B5 B6 S7 B8 B9 S10 B11 B12
    //  era 0 | era 1 | era 2  | era 3  | era 4   | era 5 ...
    //  where
    //   S - switch block
    //   B - non-switch block

    // If `include_switch_block_at_tip`, the additional switch block of height 13 will be added at
    // the tip of the chain.
    let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture_from_parts(
        &harness,
        None,
        Some(chainspec.protocol_version()),
        None,
        None,
        maybe_recent_era_count,
    );

    let mut trusted_validator_weights = BTreeMap::new();

    let (validator_secret_key, validator_public_key) = generate_ed25519_keypair();
    trusted_validator_weights.insert(validator_public_key, U512::from(2000000000000u64));

    let mut blocks: Vec<Block> = vec![];
    let block_count = 13 + include_switch_block_at_tip as u64;
    (0_u64..block_count).for_each(|height| {
        let is_switch = height == 0 || height % 3 == 1;
        let era_id = EraId::from(match height {
            0 => 0,
            1 => 1,
            _ => (height + 4) / 3,
        });
        let parent_hash = if height == 0 {
            BlockHash::new(Digest::default())
        } else {
            *blocks.get((height - 1) as usize).unwrap().hash()
        };

        let block = TestBlockBuilder::new()
            .era(era_id)
            .height(height)
            .protocol_version(chainspec.protocol_version())
            .parent_hash(parent_hash)
            .validator_weights(trusted_validator_weights.clone())
            .switch_block(is_switch)
            .build_versioned(&mut harness.rng);

        blocks.push(block);
    });
    blocks.iter().for_each(|block| {
        storage.put_block(block).unwrap();

        let fs = FinalitySignature::create(*block.hash(), block.era_id(), &validator_secret_key);
        assert!(fs.is_verified().is_ok());

        let mut block_signatures = BlockSignatures::new(*block.hash(), block.era_id());
        block_signatures.insert_signature(fs);

        if !non_signed_blocks.contains(&block.height()) {
            storage
                .write_finality_signatures(&block_signatures)
                .unwrap();
            storage.completed_blocks.insert(block.height());
        }
    });
    (storage, chainspec, blocks)
}

/// Storage component test fixture.
///
/// Creates a storage component in a temporary directory.
///
/// # Panics
///
/// Panics if setting up the storage fixture fails.
fn storage_fixture(harness: &ComponentHarness<UnitTestEvent>) -> Storage {
    let cfg = new_config(harness);
    Storage::new(
        &WithDir::new(harness.tmp.path(), cfg),
        None,
        ProtocolVersion::from_parts(1, 0, 0),
        EraId::default(),
        "test",
        MAX_TTL.into(),
        RECENT_ERA_COUNT,
        None,
        false,
    )
    .expect("could not create storage component fixture")
}

/// Storage component test fixture.
///
/// Creates a storage component in a temporary directory.
///
/// # Panics
///
/// Panics if setting up the storage fixture fails.
fn storage_fixture_from_parts(
    harness: &ComponentHarness<UnitTestEvent>,
    hard_reset_to_start_of_era: Option<EraId>,
    protocol_version: Option<ProtocolVersion>,
    network_name: Option<&str>,
    max_ttl: Option<TimeDiff>,
    recent_era_count: Option<u64>,
) -> Storage {
    let cfg = new_config(harness);
    Storage::new(
        &WithDir::new(harness.tmp.path(), cfg),
        hard_reset_to_start_of_era,
        protocol_version.unwrap_or(ProtocolVersion::V1_0_0),
        EraId::default(),
        network_name.unwrap_or("test"),
        max_ttl.unwrap_or(MAX_TTL).into(),
        recent_era_count.unwrap_or(RECENT_ERA_COUNT),
        None,
        false,
    )
    .expect("could not create storage component fixture from parts")
}

/// Storage component test fixture with force resync enabled.
///
/// Creates a storage component in a given temporary directory.
///
/// # Panics
///
/// Panics if setting up the storage fixture fails.
fn storage_fixture_with_force_resync(cfg: &WithDir<Config>) -> Storage {
    Storage::new(
        cfg,
        None,
        ProtocolVersion::from_parts(1, 0, 0),
        EraId::default(),
        "test",
        MAX_TTL.into(),
        RECENT_ERA_COUNT,
        None,
        true,
    )
    .expect("could not create storage component fixture")
}

/// Storage component test fixture.
///
/// Creates a storage component in a temporary directory, but with a hard reset to a specified era.
///
/// # Panics
///
/// Panics if setting up the storage fixture fails.
fn storage_fixture_with_hard_reset(
    harness: &ComponentHarness<UnitTestEvent>,
    reset_era_id: EraId,
) -> Storage {
    storage_fixture_from_parts(
        harness,
        Some(reset_era_id),
        Some(ProtocolVersion::from_parts(1, 1, 0)),
        None,
        None,
        None,
    )
}

/// Creates 3 random signatures for the given block.
fn random_signatures(rng: &mut TestRng, block_hash: BlockHash, era_id: EraId) -> BlockSignatures {
    let mut block_signatures = BlockSignatures::new(block_hash, era_id);
    for _ in 0..3 {
        let secret_key = SecretKey::random(rng);
        let signature = FinalitySignature::create(block_hash, era_id, &secret_key);
        block_signatures.insert_signature(signature);
    }
    block_signatures
}

/// Requests block header at a specific height from a storage component.
fn get_block_header_at_height(
    storage: &mut Storage,
    height: u64,
    only_from_available_block_range: bool,
) -> Option<BlockHeader> {
    storage
        .read_block_header_by_height(height, only_from_available_block_range)
        .expect("should get block")
}

/// Requests block at a specific height from a storage component.
fn get_block_at_height(storage: &mut Storage, height: u64) -> Option<Block> {
    storage
        .read_block_by_height(height)
        .expect("could not get block by height")
}

/// Loads a block from a storage component.
fn get_block(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
) -> Option<Block> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetBlock {
            block_hash,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

fn is_block_stored(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
) -> bool {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::IsBlockStored {
            block_hash,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

/// Loads a block header by height from a storage component.
/// Requesting a block header by height is required currently by the RPC
/// component.
fn get_block_header_by_height(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_height: u64,
) -> Option<BlockHeader> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetBlockHeaderByHeight {
            block_height,
            only_from_available_block_range: false,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

/// Loads a block's signatures from a storage component.
fn get_block_signatures(storage: &mut Storage, block_hash: BlockHash) -> Option<BlockSignatures> {
    let mut txn = storage.env.begin_ro_txn().unwrap();
    storage.get_block_signatures(&mut txn, &block_hash).unwrap()
}

/// Loads a set of `Transaction`s from a storage component.
///
/// Applies `into_naive` to all loaded `Transaction`s.
fn get_naive_transactions(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    transaction_hashes: Multiple<TransactionHash>,
) -> Vec<Option<Transaction>> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetTransactions {
            transaction_hashes: transaction_hashes.to_vec(),
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
        .into_iter()
        .map(|opt_twfa| opt_twfa.map(TransactionWithFinalizedApprovals::into_naive))
        .collect()
}

/// Loads a deploy with associated execution info from the storage component.
///
/// Any potential finalized approvals are discarded.
fn get_naive_transaction_and_execution_info(
    storage: &mut Storage,
    transaction_hash: TransactionHash,
) -> Option<(Transaction, Option<ExecutionInfo>)> {
    let transaction = storage.get_transaction_by_hash(transaction_hash)?;
    let execution_info = storage
        .read_execution_info(transaction.hash())
        .expect("should get info");
    Some((transaction, execution_info))
}

/// Requests the highest complete block from a storage component.
fn get_highest_complete_block(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
) -> Option<Block> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetHighestCompleteBlock { responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Requests the highest complete block header from a storage component.
fn get_highest_complete_block_header(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
) -> Option<BlockHeader> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetHighestCompleteBlockHeader { responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores a block in a storage component.
fn put_complete_block(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block: Block,
) -> bool {
    let block_height = block.height();
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutBlock {
            block: Arc::new(block),
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    harness.send_request(storage, move |responder| {
        MarkBlockCompletedRequest {
            block_height,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

// Mark block complete
fn mark_block_complete(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_height: u64,
) -> bool {
    let response = harness.send_request(storage, move |responder| {
        MarkBlockCompletedRequest {
            block_height,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores a block in a storage component.
fn put_block(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block: Arc<Block>,
) -> bool {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutBlock { block, responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores a block's signatures in a storage component.
fn put_block_signatures(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    signatures: BlockSignatures,
) -> bool {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutBlockSignatures {
            signatures,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores a `Transaction` in a storage component.
fn put_transaction(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    transaction: &Transaction,
) -> bool {
    let transaction = Arc::new(transaction.clone());
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutTransaction {
            transaction,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

fn insert_to_transaction_index(
    storage: &mut Storage,
    transaction: Transaction,
    block_hash_height_and_era: BlockHashHeightAndEra,
) -> bool {
    storage
        .transaction_hash_index
        .insert(transaction.hash(), block_hash_height_and_era)
        .is_none()
}

/// Stores execution results in a storage component.
fn put_execution_results(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
    block_height: u64,
    era_id: EraId,
    execution_results: HashMap<TransactionHash, ExecutionResult>,
) {
    harness.send_request(storage, move |responder| {
        StorageRequest::PutExecutionResults {
            block_hash: Box::new(block_hash),
            block_height,
            era_id,
            execution_results,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
}

/// Gets available block range from storage.
fn get_available_block_range(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
) -> AvailableBlockRange {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetAvailableBlockRange { responder }.into()
    });
    assert!(harness.is_idle());
    response
}

fn get_approvals_hashes(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
) -> Option<ApprovalsHashes> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetApprovalsHashes {
            block_hash,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

fn get_block_header(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
    only_from_available_block_range: bool,
) -> Option<BlockHeader> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetBlockHeader {
            block_hash,
            only_from_available_block_range,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

fn get_block_transfers(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
) -> Option<Vec<Transfer>> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetBlockTransfers {
            block_hash,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

fn get_block_signature(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
    public_key: Box<PublicKey>,
) -> Option<FinalitySignature> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetBlockSignature {
            block_hash,
            public_key,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

#[test]
fn get_block_of_non_existing_block_returns_none() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let block_hash = BlockHash::random(&mut harness.rng);
    let response = get_block(&mut harness, &mut storage, block_hash);

    assert!(response.is_none());
    assert!(harness.is_idle());
}

#[test]
fn read_block_by_height_with_available_block_range() {
    let mut harness = ComponentHarness::default();

    // Create a random block, load and store it.
    let block_33 = TestBlockBuilder::new()
        .era(1)
        .height(33)
        .protocol_version(ProtocolVersion::from_parts(1, 5, 0))
        .switch_block(true)
        .build_versioned(&mut harness.rng);

    let mut storage = storage_fixture(&harness);
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, true).is_none());

    let was_new = put_complete_block(&mut harness, &mut storage, block_33.clone());
    assert!(was_new);

    assert_eq!(
        get_block_header_at_height(&mut storage, 33, false).as_ref(),
        Some(&block_33.clone_header())
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, true).as_ref(),
        Some(&block_33.clone_header())
    );

    // Create a random block as a different height, load and store it.
    let block_14 = TestBlockBuilder::new()
        .era(1)
        .height(14)
        .protocol_version(ProtocolVersion::from_parts(1, 5, 0))
        .switch_block(false)
        .build_versioned(&mut harness.rng);

    let was_new = put_complete_block(&mut harness, &mut storage, block_14.clone());
    assert!(was_new);

    assert_eq!(
        get_block_header_at_height(&mut storage, 14, false).as_ref(),
        Some(&block_14.clone_header())
    );
    assert!(get_block_header_at_height(&mut storage, 14, true).is_none());
}

#[test]
fn can_retrieve_block_by_height() {
    let mut harness = ComponentHarness::default();

    // Create some random blocks, load and store them.
    let block_33 = TestBlockBuilder::new()
        .era(1)
        .height(33)
        .protocol_version(ProtocolVersion::from_parts(1, 5, 0))
        .switch_block(true)
        .build_versioned(&mut harness.rng);
    let block_14 = TestBlockBuilder::new()
        .era(1)
        .height(14)
        .protocol_version(ProtocolVersion::from_parts(1, 5, 0))
        .switch_block(false)
        .build_versioned(&mut harness.rng);
    let block_99 = TestBlockBuilder::new()
        .era(2)
        .height(99)
        .protocol_version(ProtocolVersion::from_parts(1, 5, 0))
        .switch_block(true)
        .build_versioned(&mut harness.rng);

    let mut storage = storage_fixture(&harness);

    // Both block at ID and highest block should return `None` initially.
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert!(get_highest_complete_block(&mut harness, &mut storage).is_none());
    assert!(get_highest_complete_block_header(&mut harness, &mut storage).is_none());
    assert!(get_block_at_height(&mut storage, 14).is_none());
    assert!(get_block_header_at_height(&mut storage, 14, false).is_none());
    assert!(get_block_at_height(&mut storage, 33).is_none());
    assert!(get_block_header_at_height(&mut storage, 33, false).is_none());
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99, false).is_none());

    // Inserting 33 changes this.
    let was_new = put_complete_block(&mut harness, &mut storage, block_33.clone());
    assert!(was_new);

    assert_eq!(
        get_highest_complete_block(&mut harness, &mut storage).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(&block_33.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert!(get_block_at_height(&mut storage, 14).is_none());
    assert!(get_block_header_at_height(&mut storage, 14, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, true).as_ref(),
        Some(&block_33.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99, false).is_none());

    // Inserting block with height 14, no change in highest.
    let was_new = put_complete_block(&mut harness, &mut storage, block_14.clone());
    assert!(was_new);

    assert_eq!(
        get_highest_complete_block(&mut harness, &mut storage).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(&block_33.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 14).as_ref(),
        Some(&block_14)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, true).as_ref(),
        None
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, false).as_ref(),
        Some(&block_14.clone_header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, false).as_ref(),
        Some(&block_33.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99, false).is_none());

    // Inserting block with height 99, changes highest.
    let was_new = put_complete_block(&mut harness, &mut storage, block_99.clone());
    // Mark block 99 as complete.
    storage.completed_blocks.insert(99);
    assert!(was_new);

    assert_eq!(
        get_highest_complete_block(&mut harness, &mut storage).as_ref(),
        Some(&block_99)
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(&block_99.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 14).as_ref(),
        Some(&block_14)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, false).as_ref(),
        Some(&block_14.clone_header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, false).as_ref(),
        Some(&block_33.clone_header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 99).as_ref(),
        Some(&block_99)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 99, false).as_ref(),
        Some(&block_99.clone_header())
    );
}

#[test]
#[should_panic(expected = "duplicate entries")]
fn different_block_at_height_is_fatal() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Create two different blocks at the same height.
    let block_44_a = TestBlockBuilder::new()
        .era(1)
        .height(44)
        .switch_block(false)
        .build_versioned(&mut harness.rng);
    let block_44_b = TestBlockBuilder::new()
        .era(1)
        .height(44)
        .switch_block(false)
        .build_versioned(&mut harness.rng);

    let was_new = put_complete_block(&mut harness, &mut storage, block_44_a.clone());
    assert!(was_new);

    let was_new = put_complete_block(&mut harness, &mut storage, block_44_a);
    assert!(was_new);

    // Putting a different block with the same height should now crash.
    put_complete_block(&mut harness, &mut storage, block_44_b);
}

#[test]
fn get_vec_of_non_existing_transaction_returns_nones() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let transaction_id = Transaction::random(&mut harness.rng).hash();
    let response = get_naive_transactions(&mut harness, &mut storage, smallvec![transaction_id]);
    assert_eq!(response, vec![None]);

    // Also verify that we can retrieve using an empty set of transaction hashes.
    let response = get_naive_transactions(&mut harness, &mut storage, smallvec![]);
    assert!(response.is_empty());
}

#[test]
fn can_retrieve_store_and_load_transactions() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Create a random deploy, store and load it.
    let transaction = Transaction::random(&mut harness.rng);

    let was_new = put_transaction(&mut harness, &mut storage, &transaction);
    let block_hash_height_and_era = BlockHashHeightAndEra::random(&mut harness.rng);
    // Insert to the deploy hash index as well so that we can perform the GET later.
    // Also check that we don't have an entry there for this deploy.
    assert!(insert_to_transaction_index(
        &mut storage,
        transaction.clone(),
        block_hash_height_and_era
    ));
    assert!(was_new, "putting transaction should have returned `true`");

    // Storing the same deploy again should work, but yield a result of `false`.
    let was_new_second_time = put_transaction(&mut harness, &mut storage, &transaction);
    assert!(
        !was_new_second_time,
        "storing transaction the second time should have returned `false`"
    );
    assert!(!insert_to_transaction_index(
        &mut storage,
        transaction.clone(),
        block_hash_height_and_era
    ));

    // Retrieve the stored transaction.
    let response =
        get_naive_transactions(&mut harness, &mut storage, smallvec![transaction.hash()]);
    assert_eq!(response, vec![Some(transaction.clone())]);

    // Finally try to get the execution info as well. Since we did not store any, we expect to get
    // the block hash and height from the indices.
    let (transaction_response, exec_info_response) =
        get_naive_transaction_and_execution_info(&mut storage, transaction.hash())
            .expect("no transaction with execution info returned");

    assert_eq!(transaction_response, transaction);
    match exec_info_response {
        Some(ExecutionInfo {
            execution_result: Some(_),
            ..
        }) => {
            panic!("We didn't store any execution info but we received it in the response.")
        }
        Some(ExecutionInfo {
            block_hash,
            block_height,
            execution_result: None,
        }) => {
            assert_eq!(block_hash_height_and_era.block_hash, block_hash);
            assert_eq!(block_hash_height_and_era.block_height, block_height);
        }
        None => panic!(
            "We stored block info in the deploy hash index but we received nothing in the response."
        ),
    }

    // Create a random transaction, store and load it.
    let transaction = Transaction::random(&mut harness.rng);

    assert!(put_transaction(&mut harness, &mut storage, &transaction));
    // Don't insert to the transaction hash index. Since we have no execution results
    // either, we should receive a `None` execution info response.
    let (transaction_response, exec_info_response) =
        get_naive_transaction_and_execution_info(&mut storage, transaction.hash())
            .expect("no transaction with execution info returned");

    assert_eq!(transaction_response, transaction);
    assert!(
        exec_info_response.is_none(),
        "We didn't store any block info in the index but we received it in the response."
    );
}

#[test]
fn should_retrieve_transactions_era_ids() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Populate the `transaction_hash_index` with 5 transactions from a block in era 1.
    let era_1_transactions: Vec<Transaction> =
        iter::repeat_with(|| Transaction::random(&mut harness.rng))
            .take(5)
            .collect();
    let block_hash_height_and_era = BlockHashHeightAndEra::new(
        BlockHash::random(&mut harness.rng),
        harness.rng.gen(),
        EraId::new(1),
    );
    for transaction in era_1_transactions.clone() {
        assert!(insert_to_transaction_index(
            &mut storage,
            transaction,
            block_hash_height_and_era
        ));
    }

    // Further populate the `transaction_hash_index` with 5 deploys from a block in era 2.
    let era_2_transactions: Vec<Transaction> =
        iter::repeat_with(|| Transaction::random(&mut harness.rng))
            .take(5)
            .collect();
    let block_hash_height_and_era = BlockHashHeightAndEra::new(
        BlockHash::random(&mut harness.rng),
        harness.rng.gen(),
        EraId::new(2),
    );
    for transaction in era_2_transactions.clone() {
        assert!(insert_to_transaction_index(
            &mut storage,
            transaction,
            block_hash_height_and_era
        ));
    }

    // Check we get an empty set for deploys not yet executed.
    let random_transaction_hashes: HashSet<TransactionHash> = iter::repeat_with(|| {
        if harness.rng.gen() {
            TransactionHash::Deploy(DeployHash::random(&mut harness.rng))
        } else {
            TransactionHash::V1(TransactionV1Hash::random(&mut harness.rng))
        }
    })
    .take(5)
    .collect();
    assert!(storage
        .get_transactions_era_ids(random_transaction_hashes.clone())
        .is_empty());

    // Check we get back only era 1 for all of the era 1 deploys and similarly for era 2 ones.
    let era_1_transaction_hashes: HashSet<_> = era_1_transactions
        .iter()
        .map(|transaction| transaction.hash())
        .collect();
    let era1: HashSet<EraId> = iter::once(EraId::new(1)).collect();
    assert_eq!(
        storage.get_transactions_era_ids(era_1_transaction_hashes.clone()),
        era1
    );
    let era_2_transaction_hashes: HashSet<_> = era_2_transactions
        .iter()
        .map(|transaction| transaction.hash())
        .collect();
    let era2: HashSet<EraId> = iter::once(EraId::new(2)).collect();
    assert_eq!(
        storage.get_transactions_era_ids(era_2_transaction_hashes.clone()),
        era2
    );

    // Check we get back both eras if we use some from each collection.
    let both_eras = vec![EraId::new(1), EraId::new(2)].into_iter().collect();
    assert_eq!(
        storage.get_transactions_era_ids(
            era_1_transaction_hashes
                .iter()
                .take(3)
                .chain(era_2_transaction_hashes.iter().take(3))
                .copied()
                .collect(),
        ),
        both_eras
    );

    // Check we get back only era 1 for era 1 deploys interspersed with unexecuted deploys, and
    // similarly for era 2 ones.
    assert_eq!(
        storage.get_transactions_era_ids(
            era_1_transaction_hashes
                .iter()
                .take(1)
                .chain(random_transaction_hashes.iter().take(3))
                .copied()
                .collect(),
        ),
        era1
    );
    assert_eq!(
        storage.get_transactions_era_ids(
            era_2_transaction_hashes
                .iter()
                .take(1)
                .chain(random_transaction_hashes.iter().take(3))
                .copied()
                .collect(),
        ),
        era2
    );

    // Check we get back both eras if we use some from each collection and also some unexecuted.
    assert_eq!(
        storage.get_transactions_era_ids(
            era_1_transaction_hashes
                .iter()
                .take(3)
                .chain(era_2_transaction_hashes.iter().take(3))
                .chain(random_transaction_hashes.iter().take(3))
                .copied()
                .collect(),
        ),
        both_eras
    );
}

#[test]
fn storing_and_loading_a_lot_of_transactions_does_not_exhaust_handles() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let total = 1000;
    let batch_size = 25;

    let mut transaction_hashes = Vec::new();

    for _ in 0..total {
        let transaction = Transaction::random(&mut harness.rng);
        transaction_hashes.push(transaction.hash());
        put_transaction(&mut harness, &mut storage, &transaction);
    }

    // Shuffle transaction hashes around to get a random order.
    transaction_hashes.as_mut_slice().shuffle(&mut harness.rng);

    // Retrieve all from storage, ensuring they are found.
    for chunk in transaction_hashes.chunks(batch_size) {
        let result =
            get_naive_transactions(&mut harness, &mut storage, chunk.iter().cloned().collect());
        assert!(result.iter().all(Option::is_some));
    }
}

#[test]
fn store_random_execution_results() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // We store results for two different blocks. Each block will have five deploys executed in it.
    let block_hash_a = BlockHash::random(&mut harness.rng);
    let block_hash_b = BlockHash::random(&mut harness.rng);

    // We collect the expected result per deploy in parallel to adding them.
    let mut expected_outcome = HashMap::new();

    fn setup_block(
        harness: &mut ComponentHarness<UnitTestEvent>,
        storage: &mut Storage,
        expected_outcome: &mut HashMap<TransactionHash, ExecutionInfo>,
        block_hash: &BlockHash,
        block_height: u64,
        era_id: EraId,
    ) {
        let transaction_count = 5;

        // Results for a single block.
        let mut block_results = HashMap::new();

        // Add deploys to block.
        for _ in 0..transaction_count {
            let transaction = Transaction::random(&mut harness.rng);

            // Store deploy.
            put_transaction(harness, storage, &transaction.clone());

            let execution_result =
                ExecutionResult::from(ExecutionResultV2::random(&mut harness.rng));
            let execution_info = ExecutionInfo {
                block_hash: *block_hash,
                block_height,
                execution_result: Some(execution_result.clone()),
            };

            // Insert deploy results for the unique block-deploy combination.
            expected_outcome.insert(transaction.hash(), execution_info);

            // Add to our expected outcome.
            block_results.insert(transaction.hash(), execution_result);
        }

        // Now we can submit the block's execution results.
        put_execution_results(
            harness,
            storage,
            *block_hash,
            block_height,
            era_id,
            block_results,
        );
    }

    setup_block(
        &mut harness,
        &mut storage,
        &mut expected_outcome,
        &block_hash_a,
        1,
        EraId::new(1),
    );

    setup_block(
        &mut harness,
        &mut storage,
        &mut expected_outcome,
        &block_hash_b,
        2,
        EraId::new(1),
    );

    // At this point, we are all set up and ready to receive results. Iterate over every deploy and
    // see if its execution-data-per-block matches our expectations.
    for (txn_hash, expected_exec_info) in expected_outcome.into_iter() {
        let (transaction, maybe_exec_info) =
            get_naive_transaction_and_execution_info(&mut storage, txn_hash)
                .expect("missing transaction");

        assert_eq!(txn_hash, transaction.hash());
        assert_eq!(maybe_exec_info, Some(expected_exec_info));
    }
}

#[test]
fn store_execution_results_twice_for_same_block_deploy_pair() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let block_hash = BlockHash::random(&mut harness.rng);
    let block_height = harness.rng.gen();
    let era_id = EraId::random(&mut harness.rng);
    let transaction = Transaction::random(&mut harness.rng);
    let transaction_hash = transaction.hash();

    put_transaction(&mut harness, &mut storage, &transaction);

    let mut exec_result_1 = HashMap::new();
    exec_result_1.insert(
        transaction_hash,
        ExecutionResult::from(ExecutionResultV2::random(&mut harness.rng)),
    );

    let mut exec_result_2 = HashMap::new();
    let new_exec_result = ExecutionResult::from(ExecutionResultV2::random(&mut harness.rng));
    exec_result_2.insert(transaction_hash, new_exec_result.clone());

    put_execution_results(
        &mut harness,
        &mut storage,
        block_hash,
        block_height,
        era_id,
        exec_result_1,
    );

    // Storing a second execution result for the same deploy on the same block should overwrite the
    // first.
    put_execution_results(
        &mut harness,
        &mut storage,
        block_hash,
        block_height,
        era_id,
        exec_result_2,
    );

    let (returned_transaction, returned_exec_info) =
        get_naive_transaction_and_execution_info(&mut storage, transaction_hash)
            .expect("missing deploy");
    let expected_exec_info = Some(ExecutionInfo {
        block_hash,
        block_height,
        execution_result: Some(new_exec_result),
    });

    assert_eq!(returned_transaction, transaction);
    assert_eq!(returned_exec_info, expected_exec_info);
}

fn prepare_exec_result_with_transfer(
    rng: &mut TestRng,
    deploy_hash: &DeployHash,
) -> (ExecutionResult, Transfer) {
    let transfer = Transfer::new(
        *deploy_hash,
        rng.gen(),
        Some(rng.gen()),
        rng.gen(),
        rng.gen(),
        rng.gen(),
        rng.gen(),
        Some(rng.gen()),
    );
    let transform = TransformEntry {
        key: Key::DeployInfo(*deploy_hash).to_formatted_string(),
        transform: Transform::WriteTransfer(transfer),
    };
    let effect = ExecutionEffect {
        operations: vec![],
        transforms: vec![transform],
    };
    let exec_result = ExecutionResult::V1(ExecutionResultV1::Success {
        effect,
        transfers: vec![],
        cost: rng.gen(),
    });
    (exec_result, transfer)
}

#[test]
fn store_identical_execution_results() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let deploy = Deploy::random_valid_native_transfer(&mut harness.rng);
    let deploy_hash = *deploy.hash();
    let block = Block::V2(
        TestBlockBuilder::new()
            .transactions(Some(&Transaction::Deploy(deploy)))
            .build(&mut harness.rng),
    );
    storage.put_block(&block).unwrap();
    let block_hash = *block.hash();

    let (exec_result, transfer) = prepare_exec_result_with_transfer(&mut harness.rng, &deploy_hash);
    let mut exec_results = HashMap::new();
    exec_results.insert(TransactionHash::from(deploy_hash), exec_result.clone());

    put_execution_results(
        &mut harness,
        &mut storage,
        block_hash,
        block.height(),
        block.era_id(),
        exec_results.clone(),
    );
    {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let retrieved_results = storage
            .get_execution_results(&mut txn, &block_hash)
            .expect("should execute get")
            .expect("should return Some");
        assert_eq!(retrieved_results.len(), 1);
        assert_eq!(retrieved_results[0].0, TransactionHash::from(deploy_hash));
        assert_eq!(retrieved_results[0].1, exec_result);
    }
    let retrieved_transfers = storage
        .get_transfers(&block_hash)
        .expect("should execute get")
        .expect("should return Some");
    assert_eq!(retrieved_transfers.len(), 1);
    assert_eq!(retrieved_transfers[0], transfer);

    // We should be fine storing the exact same result twice.
    put_execution_results(
        &mut harness,
        &mut storage,
        block_hash,
        block.height(),
        block.era_id(),
        exec_results,
    );
    {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let retrieved_results = storage
            .get_execution_results(&mut txn, &block_hash)
            .expect("should execute get")
            .expect("should return Some");
        assert_eq!(retrieved_results.len(), 1);
        assert_eq!(retrieved_results[0].0, TransactionHash::from(deploy_hash));
        assert_eq!(retrieved_results[0].1, exec_result);
    }
    let retrieved_transfers = storage
        .get_transfers(&block_hash)
        .expect("should execute get")
        .expect("should return Some");
    assert_eq!(retrieved_transfers.len(), 1);
    assert_eq!(retrieved_transfers[0], transfer);
}

/// This is a regression test for the issue where `Transfer`s under a block with no deploys could be
/// returned as `None` rather than the expected `Some(vec![])`.  The fix should ensure that if no
/// Transfers are found, storage will respond with an empty collection and store the correct value
/// for future requests.
///
/// See https://github.com/casper-network/casper-node/issues/4255 for further info.
#[test]
fn should_provide_transfers_if_not_stored() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let block_v2 = TestBlockBuilder::new()
        .transactions(None)
        .build(&mut harness.rng);
    assert_eq!(block_v2.all_transactions().count(), 0);
    let block = Block::V2(block_v2);
    storage.put_block(&block).unwrap();
    let block_hash = *block.hash();

    // Check an empty collection is returned.
    let retrieved_transfers = storage
        .get_transfers(&block_hash)
        .expect("should execute get")
        .expect("should return Some");
    assert!(retrieved_transfers.is_empty());

    // Check the empty collection has been stored.
    let mut txn = storage.env.begin_ro_txn().unwrap();
    let maybe_transfers = txn
        .get_value::<_, Vec<Transfer>>(storage.transfer_db, &block_hash)
        .unwrap();
    assert_eq!(Some(vec![]), maybe_transfers);
}

/// This is a regression test for the issue where a valid collection of `Transfer`s under a given
/// block could be erroneously replaced with an empty collection.  The fix should ensure that if an
/// empty collection of Transfers is found, storage will replace it with the correct collection and
/// store the correct value for future requests.
///
/// See https://github.com/casper-network/casper-node/issues/4268 for further info.
#[test]
fn should_provide_transfers_after_emptied() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let deploy = Deploy::random_valid_native_transfer(&mut harness.rng);
    let deploy_hash = *deploy.hash();
    let block = Block::V2(
        TestBlockBuilder::new()
            .transactions(Some(&Transaction::Deploy(deploy)))
            .build(&mut harness.rng),
    );
    storage.put_block(&block).unwrap();
    let block_hash = *block.hash();

    let (exec_result, transfer) = prepare_exec_result_with_transfer(&mut harness.rng, &deploy_hash);
    let mut exec_results = HashMap::new();
    exec_results.insert(TransactionHash::from(deploy_hash), exec_result);

    put_execution_results(
        &mut harness,
        &mut storage,
        block_hash,
        block.height(),
        block.era_id(),
        exec_results.clone(),
    );
    // Replace the valid collection with an empty one.
    {
        let mut txn = storage.env.begin_rw_txn().unwrap();
        txn.put_value(
            storage.transfer_db,
            &block_hash,
            &Vec::<Transfer>::new(),
            true,
        )
        .unwrap();
        txn.commit().unwrap();
    }

    // Check the correct value is returned.
    let retrieved_transfers = storage
        .get_transfers(&block_hash)
        .expect("should execute get")
        .expect("should return Some");
    assert_eq!(retrieved_transfers.len(), 1);
    assert_eq!(retrieved_transfers[0], transfer);

    // Check the correct value has been stored.
    let mut txn = storage.env.begin_ro_txn().unwrap();
    let maybe_transfers = txn
        .get_value::<_, Vec<Transfer>>(storage.transfer_db, &block_hash)
        .unwrap();
    assert_eq!(Some(vec![transfer]), maybe_transfers);
}

/// Example state used in storage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StateData {
    a: Vec<u32>,
    b: i32,
}

#[test]
fn test_legacy_interface() {
    let mut harness = ComponentHarness::default();
    let storage = storage_fixture(&harness);

    let deploy = Deploy::random(&mut harness.rng);
    let was_new = storage.write_legacy_deploy(&deploy);
    assert!(was_new);

    // Ensure we get the deploy we expect.
    let result = storage
        .get_legacy_deploy(*deploy.hash())
        .expect("should get deploy");
    assert_eq!(result, Some(LegacyDeploy::from(deploy)));

    // A non-existent deploy should simply return `None`.
    assert!(storage
        .get_legacy_deploy(DeployHash::random(&mut harness.rng))
        .expect("should get deploy")
        .is_none())
}

#[test]
fn persist_blocks_txns_and_execution_info_across_instantiations() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Create some sample data.
    let transaction = Transaction::random(&mut harness.rng);
    let block: Block = TestBlockBuilder::new()
        .transactions(Some(&transaction))
        .build_versioned(&mut harness.rng);

    let block_height = block.height();
    let execution_result = ExecutionResult::from(ExecutionResultV2::random(&mut harness.rng));
    put_transaction(&mut harness, &mut storage, &transaction);
    put_complete_block(&mut harness, &mut storage, block.clone());
    let mut execution_results = HashMap::new();
    execution_results.insert(transaction.hash(), execution_result.clone());
    put_execution_results(
        &mut harness,
        &mut storage,
        *block.hash(),
        block.height(),
        block.era_id(),
        execution_results,
    );
    assert_eq!(
        get_block_at_height(&mut storage, block_height).expect("block not indexed properly"),
        block
    );

    // After storing everything, destroy the harness and component, then rebuild using the
    // same directory as backing.
    let (on_disk, rng) = harness.into_parts();
    let mut harness = ComponentHarness::builder()
        .on_disk(on_disk)
        .rng(rng)
        .build();
    let mut storage = storage_fixture(&harness);

    let actual_block = get_block(&mut harness, &mut storage, *block.hash())
        .expect("missing block we stored earlier");
    assert_eq!(actual_block, block);
    let actual_txns =
        get_naive_transactions(&mut harness, &mut storage, smallvec![transaction.hash()]);
    assert_eq!(actual_txns, vec![Some(transaction.clone())]);

    let (_, maybe_exec_info) =
        get_naive_transaction_and_execution_info(&mut storage, transaction.hash())
            .expect("missing deploy we stored earlier");

    let retrieved_execution_result = maybe_exec_info
        .expect("should have execution info")
        .execution_result
        .expect("should have execution result");
    assert_eq!(retrieved_execution_result, execution_result);

    assert_eq!(
        get_block_at_height(&mut storage, block_height).expect("block index was not restored"),
        block
    );
}

#[test]
fn should_hard_reset() {
    let blocks_count = 8_usize;
    let blocks_per_era = 3;
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let random_txns: Vec<_> = iter::repeat_with(|| Transaction::random(&mut harness.rng))
        .take(blocks_count)
        .collect();

    // Create and store 8 blocks, 0-2 in era 0, 3-5 in era 1, and 6,7 in era 2.
    let blocks: Vec<_> = (0..blocks_count)
        .map(|height| {
            let is_switch = height % blocks_per_era == blocks_per_era - 1;
            TestBlockBuilder::new()
                .era(height as u64 / 3)
                .height(height as u64)
                .switch_block(is_switch)
                .transactions(iter::once(
                    &random_txns.get(height).expect("should_have_deploy").clone(),
                ))
                .build_versioned(&mut harness.rng)
        })
        .collect();

    for block in &blocks {
        assert!(put_complete_block(
            &mut harness,
            &mut storage,
            block.clone()
        ));
    }

    // Create and store signatures for these blocks.
    for block in &blocks {
        let block_signatures = random_signatures(&mut harness.rng, *block.hash(), block.era_id());
        assert!(put_block_signatures(
            &mut harness,
            &mut storage,
            block_signatures
        ));
    }

    // Add execution results to deploys; deploy 0 will be executed in block 0, deploy 1 in block 1,
    // and so on.
    let mut transactions = vec![];
    let mut execution_results = vec![];
    for (index, (block_hash, block_height, era_id)) in blocks
        .iter()
        .map(|block| (block.hash(), block.height(), block.era_id()))
        .enumerate()
    {
        let transaction = random_txns.get(index).expect("should have deploys");
        let execution_result = ExecutionResult::from(ExecutionResultV2::random(&mut harness.rng));
        put_transaction(&mut harness, &mut storage, &transaction.clone());
        let mut exec_results = HashMap::new();
        exec_results.insert(transaction.hash(), execution_result);
        put_execution_results(
            &mut harness,
            &mut storage,
            *block_hash,
            block_height,
            era_id,
            exec_results.clone(),
        );
        transactions.push(transaction);
        execution_results.push(exec_results);
    }

    // Check the highest block is #7.
    assert_eq!(
        Some(blocks[blocks_count - 1].clone()),
        get_highest_complete_block(&mut harness, &mut storage)
    );

    // The closure doing the actual checks.
    let mut check = |reset_era: usize| {
        // Initialize a new storage with a hard reset to the given era, deleting blocks from that
        // era onwards.
        let mut storage = storage_fixture_with_hard_reset(&harness, EraId::from(reset_era as u64));

        // Check highest block is the last from the previous era, or `None` if resetting to era 0.
        let highest_block = get_highest_complete_block(&mut harness, &mut storage);
        if reset_era > 0 {
            assert_eq!(
                blocks[blocks_per_era * reset_era - 1].clone(),
                highest_block.unwrap()
            );
        } else {
            assert!(highest_block.is_none());
        }

        // Check deleted blocks can't be retrieved.
        for (index, block) in blocks.iter().enumerate() {
            let result = get_block(&mut harness, &mut storage, *block.hash());
            let should_get_block = index < blocks_per_era * reset_era;
            assert_eq!(should_get_block, result.is_some());
        }

        // Check signatures of deleted blocks can't be retrieved.
        for (index, block) in blocks.iter().enumerate() {
            let result = get_block_signatures(&mut storage, *block.hash());
            let should_get_sigs = index < blocks_per_era * reset_era;
            assert_eq!(should_get_sigs, result.is_some());
        }

        // Check execution results in deleted blocks have been removed.
        for (index, transaction) in transactions.iter().enumerate() {
            let (_, maybe_exec_info) =
                get_naive_transaction_and_execution_info(&mut storage, transaction.hash()).unwrap();
            let should_have_exec_results = index < blocks_per_era * reset_era;
            match maybe_exec_info {
                Some(ExecutionInfo {
                    execution_result, ..
                }) => {
                    assert_eq!(should_have_exec_results, execution_result.is_some());
                }
                None => assert!(!should_have_exec_results),
            };
        }
    };

    // Test with a hard reset to era 2, deleting blocks (and associated data) 6 and 7.
    check(2);
    // Test with a hard reset to era 1, further deleting blocks (and associated data) 3, 4 and 5.
    check(1);
    // Test with a hard reset to era 0, deleting all blocks and associated data.
    check(0);
}

#[test]
fn should_create_subdir_named_after_network() {
    let harness = ComponentHarness::default();
    let cfg = new_config(&harness);

    let network_name = "test";
    let storage = Storage::new(
        &WithDir::new(harness.tmp.path(), cfg.clone()),
        None,
        ProtocolVersion::from_parts(1, 0, 0),
        EraId::default(),
        network_name,
        MAX_TTL.into(),
        RECENT_ERA_COUNT,
        None,
        false,
    )
    .unwrap();

    let expected_path = cfg.path.join(network_name);

    assert!(expected_path.exists());
    assert_eq!(expected_path, storage.root_path());
}

#[test]
fn should_not_try_to_move_nonexistent_files() {
    let harness = ComponentHarness::default();
    let cfg = new_config(&harness);
    let file_names = ["temp.txt"];

    let expected = should_move_storage_files_to_network_subdir(&cfg.path, &file_names).unwrap();

    assert!(!expected);
}

#[test]
fn should_move_files_if_they_exist() {
    let harness = ComponentHarness::default();
    let cfg = new_config(&harness);
    let file_names = ["temp1.txt", "temp2.txt", "temp3.txt"];

    // Storage will create this in the constructor,
    // doing this manually since we're not calling the constructor in this test.
    fs::create_dir(cfg.path.clone()).unwrap();

    // create empty files for testing.
    File::create(cfg.path.join(file_names[0])).unwrap();
    File::create(cfg.path.join(file_names[1])).unwrap();
    File::create(cfg.path.join(file_names[2])).unwrap();

    let expected = should_move_storage_files_to_network_subdir(&cfg.path, &file_names).unwrap();

    assert!(expected);
}

#[test]
fn should_return_error_if_files_missing() {
    let harness = ComponentHarness::default();
    let cfg = new_config(&harness);
    let file_names = ["temp1.txt", "temp2.txt", "temp3.txt"];

    // Storage will create this in the constructor,
    // doing this manually since we're not calling the constructor in this test.
    fs::create_dir(cfg.path.clone()).unwrap();

    // create empty files for testing, but not all of the files.
    File::create(cfg.path.join(file_names[1])).unwrap();
    File::create(cfg.path.join(file_names[2])).unwrap();

    let actual = should_move_storage_files_to_network_subdir(&cfg.path, &file_names);

    assert!(actual.is_err());
}

#[test]
fn should_actually_move_specified_files() {
    let harness = ComponentHarness::default();
    let cfg = new_config(&harness);
    let file_names = ["temp1.txt", "temp2.txt", "temp3.txt"];
    let root = cfg.path;
    let subdir = root.join("test");
    let src_path1 = root.join(file_names[0]);
    let src_path2 = root.join(file_names[1]);
    let src_path3 = root.join(file_names[2]);
    let dest_path1 = subdir.join(file_names[0]);
    let dest_path2 = subdir.join(file_names[1]);
    let dest_path3 = subdir.join(file_names[2]);

    // Storage will create this in the constructor,
    // doing this manually since we're not calling the constructor in this test.
    fs::create_dir_all(subdir.clone()).unwrap();

    // create empty files for testing.
    File::create(src_path1.clone()).unwrap();
    File::create(src_path2.clone()).unwrap();
    File::create(src_path3.clone()).unwrap();

    assert!(src_path1.exists());
    assert!(src_path2.exists());
    assert!(src_path3.exists());

    let result = move_storage_files_to_network_subdir(&root, &subdir, &file_names);

    assert!(result.is_ok());
    assert!(!src_path1.exists());
    assert!(!src_path2.exists());
    assert!(!src_path3.exists());
    assert!(dest_path1.exists());
    assert!(dest_path2.exists());
    assert!(dest_path3.exists());
}

#[test]
fn can_put_and_get_block() {
    let mut harness = ComponentHarness::default();

    // This test is not restricted by the block availability index.
    let only_from_available_block_range = false;

    // Create a random block, store and load it.
    let block = TestBlockBuilder::new().build(&mut harness.rng);

    let mut storage = storage_fixture(&harness);

    let was_new = put_complete_block(&mut harness, &mut storage, block.clone().into());
    assert!(was_new, "putting block should have returned `true`");

    // Storing the same block again should work, but yield a result of `true`.
    let was_new_second_time = put_complete_block(&mut harness, &mut storage, block.clone().into());
    assert!(
        was_new_second_time,
        "storing block the second time should have returned `true`"
    );

    let response =
        get_block(&mut harness, &mut storage, *block.hash()).expect("should get response");
    let response: BlockV2 = response.try_into().expect("should get BlockV2");
    assert_eq!(response, block);

    // Also ensure we can retrieve just the header.
    let response = harness.send_request(&mut storage, |responder| {
        StorageRequest::GetBlockHeader {
            block_hash: *block.hash(),
            only_from_available_block_range,
            responder,
        }
        .into()
    });

    assert_eq!(response.as_ref(), Some(&block.header().clone().into()));
}

#[test]
fn should_get_trusted_ancestor_headers() {
    let (storage, _, blocks) = create_sync_leap_test_chain(&[], false, None);

    let get_results = |requested_height: usize| -> Vec<u64> {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let requested_block_header = blocks.get(requested_height).unwrap().clone_header();
        storage
            .get_trusted_ancestor_headers(&mut txn, &requested_block_header)
            .unwrap()
            .unwrap()
            .iter()
            .map(|block_header| block_header.height())
            .collect()
    };

    assert_eq!(get_results(7), &[6, 5, 4]);
    assert_eq!(get_results(9), &[8, 7]);
    assert_eq!(get_results(5), &[4]);
}

#[test]
fn should_get_signed_block_headers() {
    let (storage, _, blocks) = create_sync_leap_test_chain(&[], false, None);

    let get_results = |requested_height: usize| -> Vec<u64> {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let requested_block_header = blocks.get(requested_height).unwrap().clone_header();
        let highest_block_header_with_sufficient_signatures = storage
            .get_highest_complete_signed_block_header(&mut txn)
            .unwrap()
            .unwrap();
        storage
            .get_signed_block_headers(
                &mut txn,
                &requested_block_header,
                &highest_block_header_with_sufficient_signatures,
            )
            .unwrap()
            .unwrap()
            .iter()
            .map(|signed_block_header| signed_block_header.block_header().height())
            .collect()
    };

    assert!(
        get_results(12).is_empty(),
        "should return empty set if asked for a most recent signed block"
    );
    assert_eq!(get_results(5), &[7, 10, 12]);
    assert_eq!(get_results(2), &[4, 7, 10, 12]);
    assert_eq!(get_results(1), &[4, 7, 10, 12]);
    assert_eq!(
        get_results(10),
        &[12],
        "should return only tip if asked for a most recent switch block"
    );
    assert_eq!(
        get_results(7),
        &[10, 12],
        "should not include switch block that was directly requested"
    );
}

#[test]
fn should_get_signed_block_headers_when_no_sufficient_finality_in_most_recent_block() {
    let (storage, _, blocks) = create_sync_leap_test_chain(&[12], false, None);

    let get_results = |requested_height: usize| -> Vec<u64> {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let requested_block_header = blocks.get(requested_height).unwrap().clone_header();
        let highest_block_header_with_sufficient_signatures = storage
            .get_highest_complete_signed_block_header(&mut txn)
            .unwrap()
            .unwrap();

        storage
            .get_signed_block_headers(
                &mut txn,
                &requested_block_header,
                &highest_block_header_with_sufficient_signatures,
            )
            .unwrap()
            .unwrap()
            .iter()
            .map(|signed_block_header| signed_block_header.block_header().height())
            .collect()
    };

    assert!(
        get_results(11).is_empty(),
        "should return empty set if asked for a most recent signed block",
    );
    assert_eq!(get_results(5), &[7, 10, 11]);
    assert_eq!(get_results(2), &[4, 7, 10, 11]);
    assert_eq!(get_results(1), &[4, 7, 10, 11]);
    assert_eq!(
        get_results(10),
        &[11],
        "should return only tip if asked for a most recent switch block"
    );
    assert_eq!(
        get_results(7),
        &[10, 11],
        "should not include switch block that was directly requested"
    );
}

#[test]
fn should_get_sync_leap() {
    let (storage, chainspec, blocks) = create_sync_leap_test_chain(&[], false, None);

    let requested_block_hash = *blocks.get(6).unwrap().hash();
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(requested_block_hash);
    let sync_leap_result = storage.get_sync_leap(sync_leap_identifier).unwrap();

    let sync_leap = match sync_leap_result {
        FetchResponse::Fetched(sync_leap) => sync_leap,
        _ => panic!("should have leap sync"),
    };

    assert_eq!(sync_leap.trusted_block_header.height(), 6);
    assert_eq!(
        block_headers_into_heights(&sync_leap.trusted_ancestor_headers),
        vec![5, 4],
    );
    assert_eq!(
        signed_block_headers_into_heights(&sync_leap.signed_block_headers),
        vec![7, 10, 12]
    );

    sync_leap
        .validate(&SyncLeapValidationMetaData::from_chainspec(&chainspec))
        .unwrap();
}

#[test]
fn sync_leap_signed_block_headers_should_be_empty_when_asked_for_a_tip() {
    let (storage, chainspec, blocks) = create_sync_leap_test_chain(&[], false, None);

    let requested_block_hash = *blocks.get(12).unwrap().hash();
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(requested_block_hash);
    let sync_leap_result = storage.get_sync_leap(sync_leap_identifier).unwrap();

    let sync_leap = match sync_leap_result {
        FetchResponse::Fetched(sync_leap) => sync_leap,
        _ => panic!("should have leap sync"),
    };

    assert_eq!(sync_leap.trusted_block_header.height(), 12);
    assert_eq!(
        block_headers_into_heights(&sync_leap.trusted_ancestor_headers),
        vec![11, 10],
    );
    assert!(signed_block_headers_into_heights(&sync_leap.signed_block_headers).is_empty());

    sync_leap
        .validate(&SyncLeapValidationMetaData::from_chainspec(&chainspec))
        .unwrap();
}

#[test]
fn sync_leap_should_populate_trusted_ancestor_headers_if_tip_is_a_switch_block() {
    let (storage, chainspec, blocks) = create_sync_leap_test_chain(&[], true, None);

    let requested_block_hash = *blocks.get(13).unwrap().hash();
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(requested_block_hash);
    let sync_leap_result = storage.get_sync_leap(sync_leap_identifier).unwrap();

    let sync_leap = match sync_leap_result {
        FetchResponse::Fetched(sync_leap) => sync_leap,
        _ => panic!("should have leap sync"),
    };

    assert_eq!(sync_leap.trusted_block_header.height(), 13);
    assert_eq!(
        block_headers_into_heights(&sync_leap.trusted_ancestor_headers),
        vec![12, 11, 10],
    );
    assert!(signed_block_headers_into_heights(&sync_leap.signed_block_headers).is_empty());

    sync_leap
        .validate(&SyncLeapValidationMetaData::from_chainspec(&chainspec))
        .unwrap();
}

#[test]
fn should_respect_allowed_era_diff_in_get_sync_leap() {
    let maybe_recent_era_count = Some(1);
    let (storage, _, blocks) = create_sync_leap_test_chain(&[], false, maybe_recent_era_count);

    let requested_block_hash = *blocks.get(6).unwrap().hash();
    let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(requested_block_hash);
    let sync_leap_result = storage.get_sync_leap(sync_leap_identifier).unwrap();

    assert!(
        matches!(sync_leap_result, FetchResponse::NotProvided(_)),
        "should not have sync leap"
    );
}

#[test]
fn should_restrict_returned_blocks() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Create the following disjoint sequences: 1-2 4-5
    IntoIterator::into_iter([1, 2, 4, 5]).for_each(|height| {
        let block = TestBlockBuilder::new()
            .era(1)
            .height(height)
            .protocol_version(ProtocolVersion::from_parts(1, 5, 0))
            .switch_block(false)
            .build_versioned(&mut harness.rng);

        storage.put_block(&block).unwrap();
        storage.completed_blocks.insert(height);
    });

    // Without restriction, the node should attempt to return any requested block
    // regardless if it is in the disjoint sequences.
    assert!(storage
        .should_return_block(0, false)
        .expect("should return block failed"));
    assert!(storage
        .should_return_block(1, false)
        .expect("should return block failed"));
    assert!(storage
        .should_return_block(2, false)
        .expect("should return block failed"));
    assert!(storage
        .should_return_block(3, false)
        .expect("should return block failed"));
    assert!(storage
        .should_return_block(4, false)
        .expect("should return block failed"));
    assert!(storage
        .should_return_block(5, false)
        .expect("should return block failed"));
    assert!(storage
        .should_return_block(6, false)
        .expect("should return block failed"));

    // With restriction, the node should attempt to return only the blocks that are
    // on the highest disjoint sequence, i.e blocks 4 and 5 only.
    assert!(!storage
        .should_return_block(0, true)
        .expect("should return block failed"));
    assert!(!storage
        .should_return_block(1, true)
        .expect("should return block failed"));
    assert!(!storage
        .should_return_block(2, true)
        .expect("should return block failed"));
    assert!(!storage
        .should_return_block(3, true)
        .expect("should return block failed"));
    assert!(storage
        .should_return_block(4, true)
        .expect("should return block failed"));
    assert!(storage
        .should_return_block(5, true)
        .expect("should return block failed"));
    assert!(!storage
        .should_return_block(6, true)
        .expect("should return block failed"));
}

#[test]
fn should_get_block_header_by_height() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let block = TestBlockBuilder::new().build_versioned(&mut harness.rng);
    let expected_header = block.clone_header();
    let height = block.height();

    // Requesting the block header before it is in storage should return None.
    assert!(get_block_header_by_height(&mut harness, &mut storage, height).is_none());

    let was_new = put_complete_block(&mut harness, &mut storage, block);
    assert!(was_new);

    // Requesting the block header after it is in storage should return the block header.
    let maybe_block_header = get_block_header_by_height(&mut harness, &mut storage, height);
    assert!(maybe_block_header.is_some());
    assert_eq!(expected_header, maybe_block_header.unwrap());
}

#[ignore]
#[test]
fn check_force_resync_with_marker_file() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);
    let cfg = WithDir::new(harness.tmp.path(), new_config(&harness));
    let force_resync_file_path = storage.root_path().join(FORCE_RESYNC_FILE_NAME);
    assert!(!force_resync_file_path.exists());

    // Add a couple of blocks into storage.
    let first_block = TestBlockBuilder::new().build_versioned(&mut harness.rng);
    put_complete_block(&mut harness, &mut storage, first_block.clone());
    let second_block = loop {
        // We need to make sure that the second random block has different height than the first
        // one.
        let block = TestBlockBuilder::new().build_versioned(&mut harness.rng);
        if block.height() != first_block.height() {
            break block;
        }
    };
    put_complete_block(&mut harness, &mut storage, second_block);
    // Make sure the completed blocks are not the default anymore.
    assert_ne!(
        storage.get_available_block_range(),
        AvailableBlockRange::RANGE_0_0
    );
    storage.persist_completed_blocks().unwrap();
    drop(storage);

    // The force resync marker file should not exist yet.
    assert!(!force_resync_file_path.exists());
    // Reinitialize storage with force resync enabled.
    let mut storage = storage_fixture_with_force_resync(&cfg);
    // The marker file should be there now.
    assert!(force_resync_file_path.exists());
    // Completed blocks has now been defaulted.
    assert_eq!(
        storage.get_available_block_range(),
        AvailableBlockRange::RANGE_0_0
    );
    let first_block_height = first_block.height();
    // Add a block into storage.
    put_complete_block(&mut harness, &mut storage, first_block);
    assert_eq!(
        storage.get_available_block_range(),
        AvailableBlockRange::new(first_block_height, first_block_height)
    );
    storage.persist_completed_blocks().unwrap();
    drop(storage);

    // We didn't remove the marker file, so it should still be there.
    assert!(force_resync_file_path.exists());
    // Reinitialize storage with force resync enabled.
    let storage = storage_fixture_with_force_resync(&cfg);
    assert!(force_resync_file_path.exists());
    // The completed blocks didn't default this time as the marker file was
    // present.
    assert_eq!(
        storage.get_available_block_range(),
        AvailableBlockRange::new(first_block_height, first_block_height)
    );
    drop(storage);
    // Remove the marker file.
    std::fs::remove_file(&force_resync_file_path).unwrap();
    assert!(!force_resync_file_path.exists());

    // Reinitialize storage with force resync enabled.
    let storage = storage_fixture_with_force_resync(&cfg);
    // The marker file didn't exist, so it was created.
    assert!(force_resync_file_path.exists());
    // Completed blocks was defaulted again.
    assert_eq!(
        storage.get_available_block_range(),
        AvailableBlockRange::RANGE_0_0
    );
}

#[test]
fn should_read_legacy_unbonding_purse() {
    // These bytes represent the `UnbondingPurse` struct with the `new_validator` field removed
    // and serialized with `bincode`.
    // In theory, we can generate these bytes by serializing the `WithdrawPurse`, but at some point,
    // these two structs may diverge and it's a safe bet to rely on the bytes
    // that are consistent with what we keep in the current storage.
    const LEGACY_BYTES: &str = "0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e07010000002000000000000000197f6b23e16c8532c6abc838facd5ea789be0c76b2920334039bfa8b3d368d610100000020000000000000004508a07aa941707f3eb2db94c8897a80b2c1197476b6de213ac273df7d86c4ffffffffffffffffff40feffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    let decoded = base16::decode(LEGACY_BYTES).expect("decode");
    let deserialized: UnbondingPurse = deserialize_internal(&decoded)
        .expect("should deserialize w/o error")
        .expect("should be Some");

    // Make sure the new field is set to default.
    assert_eq!(*deserialized.new_validator(), Option::default())
}

#[test]
fn unbonding_purse_serialization_roundtrip() {
    let original = UnbondingPurse::new(
        URef::new([14; 32], AccessRights::READ_ADD_WRITE),
        {
            let secret_key =
                SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
            PublicKey::from(&secret_key)
        },
        {
            let secret_key =
                SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap();
            PublicKey::from(&secret_key)
        },
        EraId::MAX,
        U512::max_value() - 1,
        Some({
            let secret_key =
                SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap();
            PublicKey::from(&secret_key)
        }),
    );

    let serialized = serialize_internal(&original).expect("serialization");
    let deserialized: UnbondingPurse = deserialize_internal(&serialized)
        .expect("should deserialize w/o error")
        .expect("should be Some");

    assert_eq!(original, deserialized);

    // Explicitly assert that the `new_validator` is not `None`
    assert!(deserialized.new_validator().is_some())
}

// Clippy complains because there's a `OnceCell` in `FinalitySignature`, hence it should not be used
// as a key in `BTreeSet`. However, we don't change the content of the cell during the course of the
// test so there's no risk the hash or order of keys will change.
#[allow(clippy::mutable_key_type)]
#[track_caller]
fn assert_signatures(storage: &Storage, block_hash: BlockHash, expected: Vec<FinalitySignature>) {
    let mut txn = storage.env.begin_ro_txn().unwrap();
    let actual = storage
        .get_block_signatures(&mut txn, &block_hash)
        .expect("should be able to read signatures");
    let actual = actual.map_or(BTreeSet::new(), |signatures| {
        signatures.finality_signatures().collect()
    });
    let expected: BTreeSet<_> = expected.into_iter().collect();
    assert_eq!(actual, expected);
}

#[test]
fn should_initialize_block_metadata_db() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let block_1 = TestBlockBuilder::new().build(&mut harness.rng);
    let fs_1_1 = FinalitySignature::random_for_block(
        *block_1.hash(),
        block_1.header().era_id(),
        &mut harness.rng,
    );
    let fs_1_2 = FinalitySignature::random_for_block(
        *block_1.hash(),
        block_1.header().era_id(),
        &mut harness.rng,
    );

    let block_2 = TestBlockBuilder::new().build(&mut harness.rng);
    let fs_2_1 = FinalitySignature::random_for_block(
        *block_2.hash(),
        block_2.header().era_id(),
        &mut harness.rng,
    );
    let fs_2_2 = FinalitySignature::random_for_block(
        *block_2.hash(),
        block_2.header().era_id(),
        &mut harness.rng,
    );

    let block_3 = TestBlockBuilder::new().build(&mut harness.rng);
    let fs_3_1 = FinalitySignature::random_for_block(
        *block_3.hash(),
        block_3.header().era_id(),
        &mut harness.rng,
    );
    let fs_3_2 = FinalitySignature::random_for_block(
        *block_3.hash(),
        block_3.header().era_id(),
        &mut harness.rng,
    );

    let block_4 = TestBlockBuilder::new().build(&mut harness.rng);

    let _ = storage.put_finality_signature(Box::new(fs_1_1.clone()));
    let _ = storage.put_finality_signature(Box::new(fs_1_2.clone()));
    let _ = storage.put_finality_signature(Box::new(fs_2_1.clone()));
    let _ = storage.put_finality_signature(Box::new(fs_2_2.clone()));
    let _ = storage.put_finality_signature(Box::new(fs_3_1.clone()));
    let _ = storage.put_finality_signature(Box::new(fs_3_2.clone()));

    assert_signatures(
        &storage,
        *block_1.hash(),
        vec![fs_1_1.clone(), fs_1_2.clone()],
    );
    assert_signatures(
        &storage,
        *block_2.hash(),
        vec![fs_2_1.clone(), fs_2_2.clone()],
    );
    assert_signatures(
        &storage,
        *block_3.hash(),
        vec![fs_3_1.clone(), fs_3_2.clone()],
    );
    assert_signatures(&storage, *block_4.hash(), vec![]);

    // Purging empty set of blocks should not change state.
    let to_be_purged = HashSet::new();
    let _ = initialize_block_metadata_db(&storage.env, storage.block_metadata_db, to_be_purged);
    assert_signatures(&storage, *block_1.hash(), vec![fs_1_1, fs_1_2]);
    assert_signatures(
        &storage,
        *block_2.hash(),
        vec![fs_2_1.clone(), fs_2_2.clone()],
    );
    assert_signatures(
        &storage,
        *block_3.hash(),
        vec![fs_3_1.clone(), fs_3_2.clone()],
    );

    // Purging for block_1 should leave sigs for block_2 and block_3 intact.
    let to_be_purged = HashSet::from_iter([*block_1.hash()]);
    let _ = initialize_block_metadata_db(&storage.env, storage.block_metadata_db, to_be_purged);
    assert_signatures(&storage, *block_1.hash(), vec![]);
    assert_signatures(
        &storage,
        *block_2.hash(),
        vec![fs_2_1.clone(), fs_2_2.clone()],
    );
    assert_signatures(
        &storage,
        *block_3.hash(),
        vec![fs_3_1.clone(), fs_3_2.clone()],
    );
    assert_signatures(&storage, *block_4.hash(), vec![]);

    // Purging for block_4 (which has no signatures) should not modify state.
    let to_be_purged = HashSet::from_iter([*block_4.hash()]);
    let _ = initialize_block_metadata_db(&storage.env, storage.block_metadata_db, to_be_purged);
    assert_signatures(&storage, *block_1.hash(), vec![]);
    assert_signatures(&storage, *block_2.hash(), vec![fs_2_1, fs_2_2]);
    assert_signatures(&storage, *block_3.hash(), vec![fs_3_1, fs_3_2]);
    assert_signatures(&storage, *block_4.hash(), vec![]);

    // Purging for all blocks should leave no signatures.
    let to_be_purged = HashSet::from_iter([
        *block_1.hash(),
        *block_2.hash(),
        *block_3.hash(),
        *block_4.hash(),
    ]);

    let _ = initialize_block_metadata_db(&storage.env, storage.block_metadata_db, to_be_purged);
    assert_signatures(&storage, *block_1.hash(), vec![]);
    assert_signatures(&storage, *block_2.hash(), vec![]);
    assert_signatures(&storage, *block_3.hash(), vec![]);
    assert_signatures(&storage, *block_4.hash(), vec![]);
}

fn copy_dir_recursive(src: impl AsRef<Path>, dest: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(&dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(entry.path(), dest.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dest.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[test]
fn can_retrieve_block_by_height_with_different_block_versions() {
    let mut harness = ComponentHarness::default();

    // BlockV1 as a versioned Block
    let block_14 = TestBlockV1Builder::new()
        .era(1)
        .height(14)
        .switch_block(false)
        .build(&mut harness.rng);

    // BlockV2 as a versioned Block
    let block_v2_33 = TestBlockBuilder::new()
        .era(1)
        .height(33)
        .switch_block(true)
        .build_versioned(&mut harness.rng);
    let block_33: Block = block_v2_33.clone();

    // BlockV2
    let block_v2_99 = TestBlockBuilder::new()
        .era(2)
        .height(99)
        .switch_block(true)
        .build_versioned(&mut harness.rng);
    let block_99: Block = block_v2_99.clone();

    let mut storage = storage_fixture(&harness);

    assert!(get_block(&mut harness, &mut storage, *block_14.hash()).is_none());
    assert!(get_block(&mut harness, &mut storage, *block_v2_33.hash()).is_none());
    assert!(get_block(&mut harness, &mut storage, *block_v2_99.hash()).is_none());
    assert!(!is_block_stored(
        &mut harness,
        &mut storage,
        *block_14.hash()
    ));
    assert!(!is_block_stored(
        &mut harness,
        &mut storage,
        *block_v2_33.hash()
    ));
    assert!(!is_block_stored(
        &mut harness,
        &mut storage,
        *block_v2_99.hash()
    ));

    let was_new = put_block(&mut harness, &mut storage, Arc::new(block_33.clone()));
    assert!(was_new);
    assert!(mark_block_complete(
        &mut harness,
        &mut storage,
        block_v2_33.height()
    ));

    // block is of the current version so it should be returned
    let block =
        get_block(&mut harness, &mut storage, *block_v2_33.hash()).expect("should have block");
    assert!(matches!(block, Block::V2(_)));

    // block is stored since it was returned before
    assert!(is_block_stored(
        &mut harness,
        &mut storage,
        *block_v2_33.hash()
    ));

    assert_eq!(
        get_highest_complete_block(&mut harness, &mut storage).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(&block_v2_33.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert!(get_block_at_height(&mut storage, 14).is_none());
    assert!(get_block_header_at_height(&mut storage, 14, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, true).as_ref(),
        Some(&block_v2_33.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99, false).is_none());

    let was_new = put_block(
        &mut harness,
        &mut storage,
        Arc::new(Block::from(block_14.clone())),
    );
    assert!(was_new);

    // block is not of the current version so don't return it
    let block = get_block(&mut harness, &mut storage, *block_14.hash()).expect("should have block");
    assert!(matches!(block, Block::V1(_)));

    // block should be stored as versioned and should be returned
    assert!(get_block(&mut harness, &mut storage, *block_14.hash()).is_some());

    // block is stored since it was returned before
    assert!(is_block_stored(
        &mut harness,
        &mut storage,
        *block_14.hash()
    ));

    assert_eq!(
        get_highest_complete_block(&mut harness, &mut storage).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(&block_v2_33.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 14).as_ref(),
        Some(&Block::from(block_14.clone()))
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, true).as_ref(),
        None
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, false).as_ref(),
        Some(&block_14.header().clone().into())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, false).as_ref(),
        Some(&block_v2_33.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99, false).is_none());

    // Inserting block with height 99, changes highest.
    let was_new = put_complete_block(&mut harness, &mut storage, block_v2_99.clone());
    // Mark block 99 as complete.
    storage.completed_blocks.insert(99);
    assert!(was_new);

    assert_eq!(
        get_highest_complete_block(&mut harness, &mut storage).as_ref(),
        Some(&(block_v2_99))
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(&block_v2_99.clone_header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 14).as_ref(),
        Some(&Block::from(block_14.clone()))
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, false).as_ref(),
        Some(&block_14.header().clone().into())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, false).as_ref(),
        Some(&block_v2_33.clone_header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 99).as_ref(),
        Some(&block_99)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 99, false).as_ref(),
        Some(&block_v2_99.clone_header())
    );
}

static TEST_STORAGE_DIR_1_5_2: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/test/storage/1.5.2/storage-1")
});
static STORAGE_INFO_FILE_NAME: &str = "storage_info.json";

#[derive(Serialize, Deserialize, Debug)]
struct Node1_5_2BlockInfo {
    height: u64,
    era: EraId,
    approvals_hashes: Option<Vec<DeployApprovalsHash>>,
    signatures: Option<BlockSignatures>,
    deploy_hashes: Vec<DeployHash>,
}

// Summary information about the context of a database
#[derive(Serialize, Deserialize, Debug)]
struct Node1_5_2StorageInfo {
    net_name: String,
    protocol_version: ProtocolVersion,
    block_range: (u64, u64),
    blocks: HashMap<BlockHash, Node1_5_2BlockInfo>,
    deploys: Vec<DeployHash>,
}

impl Node1_5_2StorageInfo {
    fn from_file(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        Ok(serde_json::from_slice(fs::read(path)?.as_slice()).expect("Malformed JSON"))
    }
}

// Use the storage component APIs to determine if a block is or is not in storage.
fn assert_block_exists_in_storage(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: &BlockHash,
    block_height: u64,
    only_from_available_block_range: bool,
    expect_exists_as_latest_version: bool,
    expect_exists_as_versioned: bool,
) {
    let expect_exists = expect_exists_as_latest_version || expect_exists_as_versioned;

    // Check if the block is stored at all
    assert_eq!(
        is_block_stored(harness, storage, *block_hash),
        expect_exists
    );

    // GetBlock should return only blocks from storage that are of the current version.
    assert_eq!(
        get_block(harness, storage, *block_hash)
            .map_or(false, |block| matches!(block, Block::V2(_))),
        expect_exists_as_latest_version
    );
    assert_eq!(
        storage
            .read_block(block_hash)
            .unwrap()
            .map_or(false, |block| matches!(block, Block::V2(_))),
        expect_exists_as_latest_version
    );

    // Check if we can get the block as a versioned Block.
    let block = get_block(harness, storage, *block_hash);
    assert_eq!(
        storage
            .read_block(block_hash)
            .unwrap()
            .map_or(false, |_| true),
        expect_exists_as_versioned
    );
    assert_eq!(block.map_or(false, |_| true), expect_exists_as_versioned);

    // Check if the header can be fetched from storage.
    assert_eq!(
        get_block_header(
            harness,
            storage,
            *block_hash,
            only_from_available_block_range
        )
        .map_or(false, |_| true),
        expect_exists
    );
    assert_eq!(
        get_block_header(harness, storage, *block_hash, false).map_or(false, |_| true),
        expect_exists
    );
    assert_eq!(
        storage
            .read_block_header(block_hash)
            .unwrap()
            .map_or(false, |_| true),
        expect_exists
    );

    assert_eq!(
        get_block_header_by_height(harness, storage, block_height).map_or(false, |_| true),
        expect_exists
    );
    assert_eq!(
        storage
            .read_block_header_by_height(block_height, only_from_available_block_range)
            .unwrap()
            .map_or(false, |_| true),
        expect_exists
    );
    assert_eq!(
        storage
            .read_block_header_by_height(block_height, false)
            .unwrap()
            .map_or(false, |_| true),
        expect_exists
    );

    if expect_exists {
        assert_eq!(
            storage
                .read_block_by_height(block_height)
                .unwrap()
                .unwrap()
                .hash(),
            block_hash
        );
        assert_eq!(
            storage
                .get_signed_block_by_hash(*block_hash, only_from_available_block_range)
                .unwrap()
                .block()
                .height(),
            block_height
        );
        assert_eq!(
            storage
                .get_signed_block_by_hash(*block_hash, false)
                .unwrap()
                .block()
                .height(),
            block_height
        );
        assert_eq!(
            storage
                .get_signed_block_by_height(block_height, only_from_available_block_range)
                .unwrap()
                .block()
                .hash(),
            block_hash
        );
        assert_eq!(
            storage
                .get_signed_block_by_height(block_height, false)
                .unwrap()
                .block()
                .hash(),
            block_hash
        );

        assert_eq!(
            storage
                .read_signed_block_by_height(block_height)
                .unwrap()
                .unwrap()
                .block()
                .hash(),
            block_hash
        );
    }
}

// Use the storage component APIs to determine if the highest stored block is the one expected.
fn assert_highest_block_in_storage(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    only_from_available_block_range: bool,
    expected_block_hash: &BlockHash,
    expected_block_height: u64,
) {
    assert_eq!(
        get_highest_complete_block_header(harness, storage)
            .unwrap()
            .height(),
        expected_block_height
    );
    assert_eq!(
        storage
            .read_highest_block_header()
            .unwrap()
            .unwrap()
            .block_hash(),
        *expected_block_hash
    );
    assert_eq!(
        storage.read_highest_block_height().unwrap(),
        expected_block_height
    );
    assert_eq!(
        get_highest_complete_block(harness, storage).unwrap().hash(),
        expected_block_hash
    );
    assert_eq!(
        storage
            .read_block_by_height(expected_block_height)
            .unwrap()
            .unwrap()
            .hash(),
        expected_block_hash
    );
    assert_eq!(
        storage.read_highest_block().unwrap().unwrap().hash(),
        expected_block_hash
    );

    if only_from_available_block_range {
        assert_eq!(
            storage
                .get_highest_signed_block(true)
                .unwrap()
                .block()
                .hash(),
            expected_block_hash
        );

        assert_eq!(
            get_highest_complete_block(harness, storage).unwrap().hash(),
            expected_block_hash
        );
        assert_eq!(
            storage
                .read_highest_complete_block()
                .unwrap()
                .unwrap()
                .hash(),
            expected_block_hash
        );
    }
    assert_eq!(
        storage
            .get_highest_signed_block(false)
            .unwrap()
            .block()
            .hash(),
        expected_block_hash
    );
}

#[test]
// Starting with node 2.0, the `Block` struct is versioned.
// Since this change impacts the storage APIs, create a test to prove that we can still access old
// unversioned blocks through the new APIs and also check that both versioned and unversioned blocks
// can co-exist in storage.
fn check_block_operations_with_node_1_5_2_storage() {
    let rng: TestRng = TestRng::new();

    let temp_dir = tempdir().unwrap();
    copy_dir_recursive(TEST_STORAGE_DIR_1_5_2.as_path(), temp_dir.path()).unwrap();
    let storage_info =
        Node1_5_2StorageInfo::from_file(temp_dir.path().join(STORAGE_INFO_FILE_NAME)).unwrap();
    let mut harness = ComponentHarness::builder()
        .on_disk(temp_dir)
        .rng(rng)
        .build();
    let mut storage = storage_fixture_from_parts(
        &harness,
        None,
        Some(ProtocolVersion::from_parts(2, 0, 0)),
        Some(storage_info.net_name.as_str()),
        None,
        None,
    );

    // Check that legacy blocks appear in the available range
    let available_range = get_available_block_range(&mut harness, &mut storage);
    assert_eq!(available_range.low(), storage_info.block_range.0);
    assert_eq!(available_range.high(), storage_info.block_range.1);

    // Check that all legacy blocks can be read as Versioned blocks with version set to V1
    for (hash, block_info) in storage_info.blocks.iter() {
        // Since all blocks in this db are V1, the blocks should exist as versioned blocks only.
        assert_block_exists_in_storage(
            &mut harness,
            &mut storage,
            hash,
            block_info.height,
            true,
            false,
            true,
        );

        // Check version
        let block = get_block(&mut harness, &mut storage, *hash).unwrap();
        assert!(matches!(block, Block::V1(_)));

        assert_eq!(block.height(), block_info.height);

        let approvals_hashes = get_approvals_hashes(&mut harness, &mut storage, *hash);
        if let Some(expected_approvals_hashes) = &block_info.approvals_hashes {
            let stored_approvals_hashes = approvals_hashes.unwrap();
            assert_eq!(
                stored_approvals_hashes.approvals_hashes().to_vec(),
                expected_approvals_hashes
                    .iter()
                    .map(|approvals_hash| TransactionApprovalsHash::Deploy(*approvals_hash))
                    .collect::<Vec<_>>()
            );
        }

        let transfers = get_block_transfers(&mut harness, &mut storage, *hash);
        if !block_info.deploy_hashes.is_empty() {
            let mut stored_transfers: Vec<DeployHash> = transfers
                .unwrap()
                .iter()
                .map(|transfer| transfer.deploy_hash)
                .collect();
            stored_transfers.sort();
            let mut expected_deploys = block_info.deploy_hashes.clone();
            expected_deploys.sort();
            assert_eq!(stored_transfers, expected_deploys);
        }

        if let Some(expected_signatures) = &block_info.signatures {
            for expected_signature in expected_signatures.finality_signatures() {
                let stored_signature = get_block_signature(
                    &mut harness,
                    &mut storage,
                    *hash,
                    Box::new(expected_signature.public_key().clone()),
                )
                .unwrap();
                assert_eq!(stored_signature, expected_signature);
            }
        }
    }

    let highest_expected_block_hash = storage_info
        .blocks
        .iter()
        .find_map(|(hash, info)| (info.height == storage_info.block_range.1).then_some(*hash))
        .unwrap();

    assert_highest_block_in_storage(
        &mut harness,
        &mut storage,
        true,
        &highest_expected_block_hash,
        storage_info.block_range.1,
    );

    assert!(storage.get_highest_signed_block(false).is_some());
    assert!(storage.get_highest_signed_block(true).is_some());
    assert!(get_highest_complete_block(&mut harness, &mut storage).is_some());
    assert!(storage.read_highest_block().unwrap().is_some());
    assert!(storage.read_highest_complete_block().unwrap().is_some());
    assert!(get_highest_complete_block_header(&mut harness, &mut storage).is_some());
    assert!(storage.read_highest_block_header().unwrap().is_some());
    assert_eq!(
        storage.read_highest_block_height().unwrap(),
        storage_info.block_range.1
    );

    let lowest_stored_block_height = *storage.block_height_index.keys().min().unwrap();

    // Now add some blocks and test if they can be retrieved correctly
    if let Some(new_lowest_height) = lowest_stored_block_height.checked_sub(1) {
        // Add a BlockV1 that precedes the lowest available block
        let new_lowest_block: Arc<Block> = Arc::new(
            TestBlockV1Builder::new()
                .era(1)
                .height(new_lowest_height)
                .switch_block(false)
                .build_versioned(&mut harness.rng),
        );

        // First check that the block doesn't exist.
        assert_block_exists_in_storage(
            &mut harness,
            &mut storage,
            new_lowest_block.hash(),
            new_lowest_height,
            false,
            false,
            false,
        );

        // Put the block to storage.
        let was_new = put_block(&mut harness, &mut storage, new_lowest_block.clone());
        assert!(was_new);

        let block_signatures = random_signatures(
            &mut harness.rng,
            *new_lowest_block.hash(),
            new_lowest_block.era_id(),
        );
        assert!(put_block_signatures(
            &mut harness,
            &mut storage,
            block_signatures
        ));

        // Check that the block was stored and can be fetched as a versioned Block.
        assert_block_exists_in_storage(
            &mut harness,
            &mut storage,
            new_lowest_block.hash(),
            new_lowest_height,
            false,
            false,
            true,
        );

        assert_eq!(
            *storage.block_height_index.keys().min().unwrap(),
            new_lowest_height
        );
    }

    {
        let new_highest_block_height = *storage.block_height_index.keys().max().unwrap() + 1;

        // Add a BlockV2 as a versioned block
        let new_highest_block: Arc<Block> = Arc::new(
            TestBlockBuilder::new()
                .era(50)
                .height(new_highest_block_height)
                .switch_block(true)
                .build_versioned(&mut harness.rng),
        );

        // First check that the block doesn't exist.
        assert_block_exists_in_storage(
            &mut harness,
            &mut storage,
            new_highest_block.hash(),
            new_highest_block_height,
            false,
            false,
            false,
        );

        let was_new = put_block(&mut harness, &mut storage, new_highest_block.clone());
        assert!(was_new);

        let block_signatures = random_signatures(
            &mut harness.rng,
            *new_highest_block.hash(),
            new_highest_block.era_id(),
        );
        assert!(put_block_signatures(
            &mut harness,
            &mut storage,
            block_signatures
        ));

        // Check that the block was stored and can be fetched as a versioned Block or
        // as a block at the latest version.
        assert_block_exists_in_storage(
            &mut harness,
            &mut storage,
            new_highest_block.hash(),
            new_highest_block_height,
            false,
            true,
            true,
        );

        assert_eq!(
            *storage.block_height_index.keys().max().unwrap(),
            new_highest_block_height
        );
    }

    {
        let new_highest_block_height = *storage.block_height_index.keys().max().unwrap() + 1;

        // Add a BlockV2 as a unversioned block
        let new_highest_block = TestBlockBuilder::new()
            .era(51)
            .height(new_highest_block_height)
            .switch_block(false)
            .build(&mut harness.rng);

        // First check that the block doesn't exist.
        assert_block_exists_in_storage(
            &mut harness,
            &mut storage,
            new_highest_block.hash(),
            new_highest_block_height,
            false,
            false,
            false,
        );

        // Insert the block and mark it complete.
        let was_new =
            put_complete_block(&mut harness, &mut storage, new_highest_block.clone().into());
        assert!(was_new);
        let block_signatures = random_signatures(
            &mut harness.rng,
            *new_highest_block.hash(),
            new_highest_block.era_id(),
        );
        assert!(put_block_signatures(
            &mut harness,
            &mut storage,
            block_signatures
        ));

        // Check that the block was stored and can be fetched as a versioned Block or
        // as a block at the latest version.
        assert_block_exists_in_storage(
            &mut harness,
            &mut storage,
            new_highest_block.hash(),
            new_highest_block_height,
            true,
            true,
            true,
        );

        assert_eq!(
            *storage.block_height_index.keys().max().unwrap(),
            new_highest_block_height
        );

        let available_range = get_available_block_range(&mut harness, &mut storage);
        assert_eq!(available_range.high(), new_highest_block_height);

        assert_highest_block_in_storage(
            &mut harness,
            &mut storage,
            true,
            new_highest_block.hash(),
            new_highest_block_height,
        );
    }
}
