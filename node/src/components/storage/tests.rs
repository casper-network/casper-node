//! Unit tests for the storage component.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, File},
    iter::{self, FromIterator},
    rc::Rc,
    sync::Arc,
};

use lmdb::Transaction;
use rand::{prelude::SliceRandom, Rng};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use casper_types::{
    generate_ed25519_keypair, system::auction::UnbondingPurse, testing::TestRng, AccessRights,
    EraId, ExecutionEffect, ExecutionResult, Key, ProtocolVersion, PublicKey, SecretKey, TimeDiff,
    Transfer, Transform, TransformEntry, URef, U512,
};

use super::{
    initialize_block_metadata_db,
    lmdb_ext::{deserialize_internal, serialize_internal, TransactionExt, WriteTransactionExt},
    move_storage_files_to_network_subdir, should_move_storage_files_to_network_subdir, Config,
    Storage, FORCE_RESYNC_FILE_NAME,
};
use crate::{
    components::fetcher::{FetchItem, FetchResponse},
    effect::{
        requests::{MarkBlockCompletedRequest, StorageRequest},
        Multiple,
    },
    testing::{ComponentHarness, UnitTestEvent},
    types::{
        sync_leap_validation_metadata::SyncLeapValidationMetaData, AvailableBlockRange, Block,
        BlockHash, BlockHashAndHeight, BlockHashHeightAndEra, BlockHeader, BlockHeaderWithMetadata,
        BlockSignatures, Chainspec, ChainspecRawBytes, Deploy, DeployHash, DeployMetadata,
        DeployMetadataExt, DeployWithFinalizedApprovals, FinalitySignature, LegacyDeploy,
        SyncLeapIdentifier, TestBlockBuilder,
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

fn signed_block_headers_into_heights(signed_block_headers: &[BlockHeaderWithMetadata]) -> Vec<u64> {
    signed_block_headers
        .iter()
        .map(|signed_block_header| signed_block_header.block_header.height())
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
    trusted_validator_weights.insert(validator_public_key.clone(), U512::from(2000000000000u64));

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
            None
        } else {
            Some(*blocks.get((height - 1) as usize).unwrap().hash())
        };

        let block = Block::random_with_specifics_and_parent_and_validator_weights(
            &mut harness.rng,
            era_id,
            height,
            chainspec.protocol_version(),
            is_switch,
            None,
            parent_hash,
            if is_switch {
                trusted_validator_weights.clone()
            } else {
                BTreeMap::new()
            },
        );

        blocks.push(block);
    });
    blocks.iter().for_each(|block| {
        storage.write_block(block).unwrap();

        let secret_rc = Rc::new(&validator_secret_key);
        let fs = FinalitySignature::create(
            *block.hash(),
            block.header().era_id(),
            &secret_rc,
            validator_public_key.clone(),
        );
        assert!(fs.is_verified().is_ok());

        let mut proofs = BTreeMap::new();
        proofs.insert(validator_public_key.clone(), fs.signature);

        let block_signatures = BlockSignatures {
            block_hash: *block.hash(),
            era_id: block.header().era_id(),
            proofs,
        };

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
fn random_signatures(rng: &mut TestRng, block: &Block) -> BlockSignatures {
    let block_hash = *block.hash();
    let era_id = block.header().era_id();
    let mut block_signatures = BlockSignatures::new(block_hash, era_id);
    for _ in 0..3 {
        let secret_key = SecretKey::random(rng);
        let signature = FinalitySignature::create(
            block_hash,
            era_id,
            &secret_key,
            PublicKey::from(&secret_key),
        );
        block_signatures.insert_proof(signature.public_key, signature.signature);
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

/// Loads a set of deploys from a storage component.
///
/// Applies `into_naive` to all loaded deploys.
fn get_naive_deploys(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    deploy_hashes: Multiple<DeployHash>,
) -> Vec<Option<Deploy>> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetDeploys {
            deploy_hashes: deploy_hashes.to_vec(),
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
        .into_iter()
        .map(|opt_dfa| opt_dfa.map(DeployWithFinalizedApprovals::into_naive))
        .collect()
}

/// Loads a deploy with associated metadata from the storage component.
///
/// Any potential finalized approvals are discarded.
fn get_naive_deploy_and_metadata(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    deploy_hash: DeployHash,
) -> Option<(Deploy, DeployMetadataExt)> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetDeployAndMetadata {
            deploy_hash,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response.map(|(deploy_with_finalized_approvals, metadata)| {
        (deploy_with_finalized_approvals.into_naive(), metadata)
    })
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
    block: Arc<Block>,
) -> bool {
    let block_height = block.height();
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutBlock { block, responder }.into()
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

/// Stores a deploy in a storage component.
fn put_deploy(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    deploy: Arc<Deploy>,
) -> bool {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutDeploy { deploy, responder }.into()
    });
    assert!(harness.is_idle());
    response
}

fn insert_to_deploy_index(
    storage: &mut Storage,
    deploy_hash: &DeployHash,
    block_hash_height_and_era: BlockHashHeightAndEra,
) -> bool {
    storage
        .deploy_hash_index
        .insert(*deploy_hash, block_hash_height_and_era)
        .is_none()
}

/// Stores execution results in a storage component.
fn put_execution_results(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
    execution_results: HashMap<DeployHash, ExecutionResult>,
) {
    harness.send_request(storage, move |responder| {
        StorageRequest::PutExecutionResults {
            block_hash: Box::new(block_hash),
            execution_results,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
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
    let block_33 = Arc::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        33,
        ProtocolVersion::from_parts(1, 5, 0),
        true,
        None,
    ));

    let mut storage = storage_fixture(&harness);
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, true).is_none());

    let was_new = put_complete_block(&mut harness, &mut storage, block_33.clone());
    assert!(was_new);

    assert_eq!(
        get_block_header_at_height(&mut storage, 33, false).as_ref(),
        Some(block_33.header())
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, true).as_ref(),
        Some(block_33.header())
    );

    // Create a random block as a different height, load and store it.
    let block_14 = Arc::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        14,
        ProtocolVersion::from_parts(1, 5, 0),
        false,
        None,
    ));

    let was_new = put_complete_block(&mut harness, &mut storage, block_14.clone());
    assert!(was_new);

    assert_eq!(
        get_block_header_at_height(&mut storage, 14, false).as_ref(),
        Some(block_14.header())
    );
    assert!(get_block_header_at_height(&mut storage, 14, true).is_none());
}

#[test]
fn can_retrieve_block_by_height() {
    let mut harness = ComponentHarness::default();

    // Create some random blocks, load and store them.
    let block_33 = Arc::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        33,
        ProtocolVersion::from_parts(1, 5, 0),
        true,
        None,
    ));
    let block_14 = Arc::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        14,
        ProtocolVersion::from_parts(1, 5, 0),
        false,
        None,
    ));
    let block_99 = Arc::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(2),
        99,
        ProtocolVersion::from_parts(1, 5, 0),
        true,
        None,
    ));

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
        Some(&*block_33)
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(block_33.header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert!(get_block_at_height(&mut storage, 14).is_none());
    assert!(get_block_header_at_height(&mut storage, 14, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, true).as_ref(),
        Some(block_33.header())
    );
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99, false).is_none());

    // Inserting block with height 14, no change in highest.
    let was_new = put_complete_block(&mut harness, &mut storage, block_14.clone());
    assert!(was_new);

    assert_eq!(
        get_highest_complete_block(&mut harness, &mut storage).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(block_33.header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 14).as_ref(),
        Some(&*block_14)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, true).as_ref(),
        None
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, false).as_ref(),
        Some(block_14.header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, false).as_ref(),
        Some(block_33.header())
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
        Some(&*block_99)
    );
    assert_eq!(
        get_highest_complete_block_header(&mut harness, &mut storage).as_ref(),
        Some(block_99.header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0, false).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 14).as_ref(),
        Some(&*block_14)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14, false).as_ref(),
        Some(block_14.header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33, false).as_ref(),
        Some(block_33.header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 99).as_ref(),
        Some(&*block_99)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 99, false).as_ref(),
        Some(block_99.header())
    );
}

#[test]
#[should_panic(expected = "duplicate entries")]
fn different_block_at_height_is_fatal() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Create two different blocks at the same height.
    let block_44_a = Arc::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        44,
        ProtocolVersion::V1_0_0,
        false,
        None,
    ));
    let block_44_b = Arc::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        44,
        ProtocolVersion::V1_0_0,
        false,
        None,
    ));

    let was_new = put_complete_block(&mut harness, &mut storage, block_44_a.clone());
    assert!(was_new);

    let was_new = put_complete_block(&mut harness, &mut storage, block_44_a);
    assert!(was_new);

    // Putting a different block with the same height should now crash.
    put_complete_block(&mut harness, &mut storage, block_44_b);
}

#[test]
fn get_vec_of_non_existing_deploy_returns_nones() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let deploy_id = DeployHash::random(&mut harness.rng);
    let response = get_naive_deploys(&mut harness, &mut storage, smallvec![deploy_id]);
    assert_eq!(response, vec![None]);

    // Also verify that we can retrieve using an empty set of deploy hashes.
    let response = get_naive_deploys(&mut harness, &mut storage, smallvec![]);
    assert!(response.is_empty());
}

#[test]
fn can_retrieve_store_and_load_deploys() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Create a random deploy, store and load it.
    let deploy = Arc::new(Deploy::random(&mut harness.rng));

    let was_new = put_deploy(&mut harness, &mut storage, Arc::clone(&deploy));
    let block_hash_height_and_era = BlockHashHeightAndEra::random(&mut harness.rng);
    // Insert to the deploy hash index as well so that we can perform the GET later.
    // Also check that we don't have an entry there for this deploy.
    assert!(insert_to_deploy_index(
        &mut storage,
        deploy.hash(),
        block_hash_height_and_era
    ));
    assert!(was_new, "putting deploy should have returned `true`");

    // Storing the same deploy again should work, but yield a result of `false`.
    let was_new_second_time = put_deploy(&mut harness, &mut storage, Arc::clone(&deploy));
    assert!(
        !was_new_second_time,
        "storing deploy the second time should have returned `false`"
    );
    assert!(!insert_to_deploy_index(
        &mut storage,
        deploy.hash(),
        block_hash_height_and_era
    ));

    // Retrieve the stored deploy.
    let response = get_naive_deploys(&mut harness, &mut storage, smallvec![*deploy.hash()]);
    assert_eq!(response, vec![Some(deploy.as_ref().clone())]);

    // Finally try to get the metadata as well. Since we did not store any, we expect to get the
    // block hash and height from the indices.
    let (deploy_response, metadata_response) = harness
        .send_request(&mut storage, |responder| {
            StorageRequest::GetDeployAndMetadata {
                deploy_hash: *deploy.hash(),
                responder,
            }
            .into()
        })
        .expect("no deploy with metadata returned");

    assert_eq!(deploy_response.into_naive(), *deploy);
    match metadata_response {
        DeployMetadataExt::Metadata(_) => {
            panic!("We didn't store any metadata but we received it in the response.")
        }
        DeployMetadataExt::BlockInfo(recv_block_hash_and_height) => {
            assert_eq!(
                BlockHashAndHeight::from(&block_hash_height_and_era),
                recv_block_hash_and_height
            )
        }
        DeployMetadataExt::Empty => panic!(
            "We stored block info in the deploy hash index \
                                            but we received nothing in the response."
        ),
    }

    // Create a random deploy, store and load it.
    let deploy = Arc::new(Deploy::random(&mut harness.rng));

    assert!(put_deploy(&mut harness, &mut storage, Arc::clone(&deploy)));
    // Don't insert to the deploy hash index. Since we have no execution results
    // either, we should receive an empty metadata response.
    let (deploy_response, metadata_response) = harness
        .send_request(&mut storage, |responder| {
            StorageRequest::GetDeployAndMetadata {
                deploy_hash: *deploy.hash(),
                responder,
            }
            .into()
        })
        .expect("no deploy with metadata returned");

    assert_eq!(deploy_response.into_naive(), *deploy);
    match metadata_response {
        DeployMetadataExt::Metadata(_) => {
            panic!("We didn't store any metadata but we received it in the response.")
        }
        DeployMetadataExt::BlockInfo(_) => {
            panic!(
                "We didn't store any block info in the index but we received it in the response."
            )
        }
        DeployMetadataExt::Empty => { /* We didn't store execution results or block info */ }
    }
}

#[test]
fn should_retrieve_deploys_era_ids() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Populate the `deploy_hash_index` with 5 deploys from a block in era 1.
    let era_1_deploy_hashes: HashSet<DeployHash> =
        iter::repeat_with(|| DeployHash::random(&mut harness.rng))
            .take(5)
            .collect();
    let block_hash_height_and_era = BlockHashHeightAndEra::new(
        BlockHash::random(&mut harness.rng),
        harness.rng.gen(),
        EraId::new(1),
    );
    for deploy_hash in &era_1_deploy_hashes {
        assert!(insert_to_deploy_index(
            &mut storage,
            deploy_hash,
            block_hash_height_and_era
        ));
    }

    // Further populate the `deploy_hash_index` with 5 deploys from a block in era 2.
    let era_2_deploy_hashes: HashSet<DeployHash> =
        iter::repeat_with(|| DeployHash::random(&mut harness.rng))
            .take(5)
            .collect();
    let block_hash_height_and_era = BlockHashHeightAndEra::new(
        BlockHash::random(&mut harness.rng),
        harness.rng.gen(),
        EraId::new(2),
    );
    for deploy_hash in &era_2_deploy_hashes {
        assert!(insert_to_deploy_index(
            &mut storage,
            deploy_hash,
            block_hash_height_and_era
        ));
    }

    // Check we get an empty set for deploys not yet executed.
    let random_deploy_hashes: HashSet<DeployHash> =
        iter::repeat_with(|| DeployHash::random(&mut harness.rng))
            .take(5)
            .collect();
    assert!(storage
        .get_deploys_era_ids(random_deploy_hashes.clone())
        .is_empty());

    // Check we get back only era 1 for all of the era 1 deploys and similarly for era 2 ones.
    let era1: HashSet<EraId> = iter::once(EraId::new(1)).collect();
    assert_eq!(
        storage.get_deploys_era_ids(era_1_deploy_hashes.clone()),
        era1
    );
    let era2: HashSet<EraId> = iter::once(EraId::new(2)).collect();
    assert_eq!(
        storage.get_deploys_era_ids(era_2_deploy_hashes.clone()),
        era2
    );

    // Check we get back both eras if we use some from each collection.
    let both_eras = vec![EraId::new(1), EraId::new(2)].into_iter().collect();
    assert_eq!(
        storage.get_deploys_era_ids(
            era_1_deploy_hashes
                .iter()
                .take(3)
                .chain(era_2_deploy_hashes.iter().take(3))
                .copied()
                .collect(),
        ),
        both_eras
    );

    // Check we get back only era 1 for era 1 deploys interspersed with unexecuted deploys, and
    // similarly for era 2 ones.
    assert_eq!(
        storage.get_deploys_era_ids(
            era_1_deploy_hashes
                .iter()
                .take(1)
                .chain(random_deploy_hashes.iter().take(3))
                .copied()
                .collect(),
        ),
        era1
    );
    assert_eq!(
        storage.get_deploys_era_ids(
            era_2_deploy_hashes
                .iter()
                .take(1)
                .chain(random_deploy_hashes.iter().take(3))
                .copied()
                .collect(),
        ),
        era2
    );

    // Check we get back both eras if we use some from each collection and also some unexecuted.
    assert_eq!(
        storage.get_deploys_era_ids(
            era_1_deploy_hashes
                .iter()
                .take(3)
                .chain(era_2_deploy_hashes.iter().take(3))
                .chain(random_deploy_hashes.iter().take(3))
                .copied()
                .collect(),
        ),
        both_eras
    );
}

#[test]
fn storing_and_loading_a_lot_of_deploys_does_not_exhaust_handles() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let total = 1000;
    let batch_size = 25;

    let mut deploy_hashes = Vec::new();

    for _ in 0..total {
        let deploy = Arc::new(Deploy::random(&mut harness.rng));
        deploy_hashes.push(*deploy.hash());
        put_deploy(&mut harness, &mut storage, deploy);
    }

    // Shuffle deploy hashes around to get a random order.
    deploy_hashes.as_mut_slice().shuffle(&mut harness.rng);

    // Retrieve all from storage, ensuring they are found.
    for chunk in deploy_hashes.chunks(batch_size) {
        let result = get_naive_deploys(&mut harness, &mut storage, chunk.iter().cloned().collect());
        assert!(result.iter().all(Option::is_some));
    }
}

#[test]
fn store_execution_results_for_two_blocks() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let deploy = Deploy::random(&mut harness.rng);

    let block_hash_a = BlockHash::random(&mut harness.rng);
    let block_hash_b = BlockHash::random(&mut harness.rng);

    // Store the deploy.
    put_deploy(&mut harness, &mut storage, Arc::new(deploy.clone()));

    // Ensure deploy exists.
    assert_eq!(
        get_naive_deploys(&mut harness, &mut storage, smallvec![*deploy.hash()]),
        vec![Some(deploy.clone())]
    );

    // Put first execution result.
    let first_result: ExecutionResult = harness.rng.gen();
    let mut first_results = HashMap::new();
    first_results.insert(*deploy.hash(), first_result.clone());
    put_execution_results(&mut harness, &mut storage, block_hash_a, first_results);

    // Retrieve and check if correct.
    let (first_deploy, first_metadata) =
        get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy.hash())
            .expect("missing on first attempt");
    assert_eq!(first_deploy, deploy);
    let mut expected_per_block_results = HashMap::new();
    expected_per_block_results.insert(block_hash_a, first_result);
    assert_eq!(
        first_metadata,
        DeployMetadata {
            execution_results: expected_per_block_results.clone()
        }
    );

    // Add second result for the same deploy, different block.
    let second_result: ExecutionResult = harness.rng.gen();
    let mut second_results = HashMap::new();
    second_results.insert(*deploy.hash(), second_result.clone());
    put_execution_results(&mut harness, &mut storage, block_hash_b, second_results);

    // Retrieve the deploy again, should now contain both.
    let (second_deploy, second_metadata) =
        get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy.hash())
            .expect("missing on second attempt");
    assert_eq!(second_deploy, deploy);
    expected_per_block_results.insert(block_hash_b, second_result);
    assert_eq!(
        second_metadata,
        DeployMetadata {
            execution_results: expected_per_block_results
        }
    );
}

#[test]
fn store_random_execution_results() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // We store results for two different blocks. Each block will have five deploys executed in it,
    // with two of these deploys being shared by both blocks, while the remaining three are unique
    // per block.
    let block_hash_a = BlockHash::random(&mut harness.rng);
    let block_hash_b = BlockHash::random(&mut harness.rng);

    // Create the shared deploys.
    let shared_deploys = vec![
        Deploy::random(&mut harness.rng),
        Deploy::random(&mut harness.rng),
    ];

    // Store shared deploys.
    for deploy in &shared_deploys {
        put_deploy(&mut harness, &mut storage, Arc::new(deploy.clone()));
    }

    // We collect the expected result per deploy in parallel to adding them.
    let mut expected_outcome = HashMap::new();

    fn setup_block(
        harness: &mut ComponentHarness<UnitTestEvent>,
        storage: &mut Storage,
        expected_outcome: &mut HashMap<DeployHash, HashMap<BlockHash, ExecutionResult>>,
        block_hash: &BlockHash,
        shared_deploys: &[Deploy],
    ) {
        let unique_count = 3;

        // Results for a single block.
        let mut block_results = HashMap::new();

        // Add three unique deploys to block.
        for _ in 0..unique_count {
            let deploy = Deploy::random(&mut harness.rng);

            // Store unique deploy.
            put_deploy(harness, storage, Arc::new(deploy.clone()));

            let execution_result: ExecutionResult = harness.rng.gen();

            // Insert deploy results for the unique block-deploy combination.
            let mut map = HashMap::new();
            map.insert(*block_hash, execution_result.clone());
            expected_outcome.insert(*deploy.hash(), map);

            // Add to our expected outcome.
            block_results.insert(*deploy.hash(), execution_result);
        }

        // Insert the shared deploys as well.
        for shared_deploy in shared_deploys {
            let execution_result: ExecutionResult = harness.rng.gen();

            // Insert the new result and ensure it is not present yet.
            let result = block_results.insert(*shared_deploy.hash(), execution_result.clone());
            assert!(result.is_none());

            // Insert into expected outcome.
            let deploy_expected = expected_outcome.entry(*shared_deploy.hash()).or_default();
            let prev = deploy_expected.insert(*block_hash, execution_result.clone());
            // Ensure we are not replacing something.
            assert!(prev.is_none());
        }

        // We should have all results for our block collected for the input.
        assert_eq!(block_results.len(), unique_count + shared_deploys.len());

        // Now we can submit the block's execution results.
        put_execution_results(harness, storage, *block_hash, block_results);
    }

    setup_block(
        &mut harness,
        &mut storage,
        &mut expected_outcome,
        &block_hash_a,
        &shared_deploys,
    );

    setup_block(
        &mut harness,
        &mut storage,
        &mut expected_outcome,
        &block_hash_b,
        &shared_deploys,
    );

    // At this point, we are all set up and ready to receive results. Iterate over every deploy and
    // see if its execution-data-per-block matches our expectations.
    for (deploy_hash, raw_meta) in expected_outcome.iter() {
        let (deploy, metadata) =
            get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy_hash)
                .expect("missing deploy");

        assert_eq!(deploy_hash, deploy.hash());

        assert_eq!(
            metadata,
            DeployMetadata {
                execution_results: raw_meta.clone()
            }
        );
    }
}

#[test]
fn store_execution_results_twice_for_same_block_deploy_pair() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let block_hash = BlockHash::random(&mut harness.rng);
    let deploy_hash = DeployHash::random(&mut harness.rng);

    let mut exec_result_1 = HashMap::new();
    exec_result_1.insert(deploy_hash, harness.rng.gen());

    let mut exec_result_2 = HashMap::new();
    exec_result_2.insert(deploy_hash, harness.rng.gen());

    put_execution_results(&mut harness, &mut storage, block_hash, exec_result_1);

    // Storing a second execution result for the same deploy on the same block should panic.
    put_execution_results(&mut harness, &mut storage, block_hash, exec_result_2);
}

fn prepare_exec_result_with_transfer(
    rng: &mut TestRng,
    deploy_hash: &DeployHash,
) -> (ExecutionResult, Transfer) {
    let transfer = Transfer::new(
        (*deploy_hash).into(),
        rng.gen(),
        Some(rng.gen()),
        rng.gen(),
        rng.gen(),
        rng.gen(),
        rng.gen(),
        Some(rng.gen()),
    );
    let transform = TransformEntry {
        key: Key::DeployInfo((*deploy_hash).into()).to_formatted_string(),
        transform: Transform::WriteTransfer(transfer),
    };
    let effect = ExecutionEffect::new(vec![transform]);
    let exec_result = ExecutionResult::Success {
        effect,
        transfers: vec![],
        cost: rng.gen(),
    };
    (exec_result, transfer)
}

#[test]
fn store_identical_execution_results() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let deploy = Deploy::random_valid_native_transfer(&mut harness.rng);
    let deploy_hash = *deploy.hash();
    let block = Block::random_with_deploys(&mut harness.rng, Some(&deploy));
    storage.write_block(&block).unwrap();
    let block_hash = *block.hash();

    let (exec_result, transfer) = prepare_exec_result_with_transfer(&mut harness.rng, &deploy_hash);
    let mut exec_results = HashMap::new();
    exec_results.insert(deploy_hash, exec_result.clone());

    put_execution_results(&mut harness, &mut storage, block_hash, exec_results.clone());
    {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let retrieved_results = storage
            .get_execution_results(&mut txn, &block_hash)
            .expect("should execute get")
            .expect("should return Some");
        assert_eq!(retrieved_results.len(), 1);
        assert_eq!(retrieved_results[0].0, deploy_hash);
        assert_eq!(retrieved_results[0].1, exec_result);
    }
    let retrieved_transfers = storage
        .get_transfers(&block_hash)
        .expect("should execute get")
        .expect("should return Some");
    assert_eq!(retrieved_transfers.len(), 1);
    assert_eq!(retrieved_transfers[0], transfer);

    // We should be fine storing the exact same result twice.
    put_execution_results(&mut harness, &mut storage, block_hash, exec_results);
    {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let retrieved_results = storage
            .get_execution_results(&mut txn, &block_hash)
            .expect("should execute get")
            .expect("should return Some");
        assert_eq!(retrieved_results.len(), 1);
        assert_eq!(retrieved_results[0].0, deploy_hash);
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

    let block = TestBlockBuilder::new()
        .deploys(None)
        .build(&mut harness.rng);
    assert_eq!(block.body().deploy_and_transfer_hashes().count(), 0);
    storage.write_block(&block).unwrap();
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
    let block = Block::random_with_deploys(&mut harness.rng, Some(&deploy));
    storage.write_block(&block).unwrap();
    let block_hash = *block.hash();

    let (exec_result, transfer) = prepare_exec_result_with_transfer(&mut harness.rng, &deploy_hash);
    let mut exec_results = HashMap::new();
    exec_results.insert(deploy_hash, exec_result);

    put_execution_results(&mut harness, &mut storage, block_hash, exec_results.clone());
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
    let mut storage = storage_fixture(&harness);

    let deploy = Arc::new(Deploy::random(&mut harness.rng));
    let was_new = put_deploy(&mut harness, &mut storage, Arc::clone(&deploy));
    assert!(was_new);

    // Ensure we get the deploy we expect.
    let result = storage
        .get_legacy_deploy(*deploy.hash())
        .expect("should get deploy");
    assert_eq!(result, Some(LegacyDeploy::from((*deploy).clone())));

    // A non-existent deploy should simply return `None`.
    assert!(storage
        .get_legacy_deploy(DeployHash::random(&mut harness.rng))
        .expect("should get deploy")
        .is_none())
}

#[test]
fn persist_blocks_deploys_and_deploy_metadata_across_instantiations() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let block = Block::random(&mut harness.rng);
    let block_height = block.height();

    // Create some sample data.
    let deploy = Deploy::random(&mut harness.rng);
    let execution_result: ExecutionResult = harness.rng.gen();
    put_deploy(&mut harness, &mut storage, Arc::new(deploy.clone()));
    put_complete_block(&mut harness, &mut storage, Arc::new(block.clone()));
    let mut execution_results = HashMap::new();
    execution_results.insert(*deploy.hash(), execution_result.clone());
    put_execution_results(&mut harness, &mut storage, *block.hash(), execution_results);
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
    let actual_deploys = get_naive_deploys(&mut harness, &mut storage, smallvec![*deploy.hash()]);
    assert_eq!(actual_deploys, vec![Some(deploy.clone())]);

    let (_, deploy_metadata_ext) =
        get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy.hash())
            .expect("missing deploy we stored earlier");

    let execution_results = match deploy_metadata_ext {
        DeployMetadataExt::Metadata(metadata) => metadata.execution_results,
        _ => panic!("Unexpected missing metadata."),
    };
    assert_eq!(execution_results.len(), 1);
    assert_eq!(execution_results[block.hash()], execution_result);

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

    let random_deploys: Vec<_> = iter::repeat_with(|| Deploy::random(&mut harness.rng))
        .take(blocks_count)
        .collect();

    // Create and store 8 blocks, 0-2 in era 0, 3-5 in era 1, and 6,7 in era 2.
    let blocks: Vec<Block> = (0..blocks_count)
        .map(|height| {
            let is_switch = height % blocks_per_era == blocks_per_era - 1;
            Block::random_with_specifics(
                &mut harness.rng,
                EraId::from(height as u64 / 3),
                height as u64,
                ProtocolVersion::V1_0_0,
                is_switch,
                iter::once(random_deploys.get(height).expect("should_have_deploy")),
            )
        })
        .collect();

    for block in &blocks {
        assert!(put_complete_block(
            &mut harness,
            &mut storage,
            Arc::new(block.clone())
        ));
    }

    // Create and store signatures for these blocks.
    for block in &blocks {
        let block_signatures = random_signatures(&mut harness.rng, block);
        assert!(put_block_signatures(
            &mut harness,
            &mut storage,
            block_signatures
        ));
    }

    // Add execution results to deploys; deploy 0 will be executed in block 0, deploy 1 in block 1,
    // and so on.
    let mut deploys = vec![];
    let mut execution_results = vec![];
    for (index, block_hash) in blocks.iter().map(|block| block.hash()).enumerate() {
        let deploy = random_deploys.get(index).expect("should have deploys");
        let execution_result: ExecutionResult = harness.rng.gen();
        put_deploy(&mut harness, &mut storage, Arc::new(deploy.clone()));
        let mut exec_results = HashMap::new();
        exec_results.insert(*deploy.hash(), execution_result);
        put_execution_results(
            &mut harness,
            &mut storage,
            *block_hash,
            exec_results.clone(),
        );
        deploys.push(deploy);
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
                blocks[blocks_per_era * reset_era - 1],
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
        for (index, deploy) in deploys.iter().enumerate() {
            let (_, deploy_metadata_ext) =
                get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy.hash()).unwrap();
            let should_have_exec_results = index < blocks_per_era * reset_era;
            match deploy_metadata_ext {
                DeployMetadataExt::Metadata(_metadata) => assert!(should_have_exec_results),
                DeployMetadataExt::BlockInfo(_block_hash_and_height) => {
                    assert!(!should_have_exec_results)
                }
                DeployMetadataExt::Empty => assert!(!should_have_exec_results),
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
    let block = Block::random(&mut harness.rng);
    let block = Arc::new(block);

    let mut storage = storage_fixture(&harness);

    let was_new = put_complete_block(&mut harness, &mut storage, block.clone());
    assert!(was_new, "putting block should have returned `true`");

    // Storing the same block again should work, but yield a result of `true`.
    let was_new_second_time = put_complete_block(&mut harness, &mut storage, block.clone());
    assert!(
        was_new_second_time,
        "storing block the second time should have returned `true`"
    );

    let response = get_block(&mut harness, &mut storage, *block.hash());
    assert_eq!(response.as_ref(), Some(&*block));

    // Also ensure we can retrieve just the header.
    let response = harness.send_request(&mut storage, |responder| {
        StorageRequest::GetBlockHeader {
            block_hash: *block.hash(),
            only_from_available_block_range,
            responder,
        }
        .into()
    });

    assert_eq!(response.as_ref(), Some(block.header()));
}

#[test]
fn should_get_trusted_ancestor_headers() {
    let (storage, _, blocks) = create_sync_leap_test_chain(&[], false, None);

    let get_results = |requested_height: usize| -> Vec<u64> {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let requested_block_header = blocks.get(requested_height).unwrap().header();
        storage
            .get_trusted_ancestor_headers(&mut txn, requested_block_header)
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
        let requested_block_header = blocks.get(requested_height).unwrap().header();
        let highest_block_header_with_sufficient_signatures = storage
            .get_header_with_metadata_of_highest_complete_block(&mut txn)
            .unwrap()
            .unwrap();
        storage
            .get_signed_block_headers(
                &mut txn,
                requested_block_header,
                &highest_block_header_with_sufficient_signatures,
            )
            .unwrap()
            .unwrap()
            .iter()
            .map(|block_header_with_metadata| block_header_with_metadata.block_header.height())
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
        let requested_block_header = blocks.get(requested_height).unwrap().header();
        let highest_block_header_with_sufficient_signatures = storage
            .get_header_with_metadata_of_highest_complete_block(&mut txn)
            .unwrap()
            .unwrap();

        storage
            .get_signed_block_headers(
                &mut txn,
                requested_block_header,
                &highest_block_header_with_sufficient_signatures,
            )
            .unwrap()
            .unwrap()
            .iter()
            .map(|block_header_with_metadata| block_header_with_metadata.block_header.height())
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

    let requested_block_hash = blocks.get(6).unwrap().header().block_hash();
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

    let requested_block_hash = blocks.get(12).unwrap().header().block_hash();
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

    let requested_block_hash = blocks.get(13).unwrap().header().block_hash();
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

    let requested_block_hash = blocks.get(6).unwrap().header().block_hash();
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
    [1, 2, 4, 5].iter().for_each(|height| {
        let block = Block::random_with_specifics(
            &mut harness.rng,
            EraId::from(1),
            *height,
            ProtocolVersion::from_parts(1, 5, 0),
            false,
            None,
        );
        storage.write_block(&block).unwrap();
        storage.completed_blocks.insert(*height);
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

    let block = Block::random(&mut harness.rng);
    let expected_header = block.header().clone();
    let height = block.height();

    // Requesting the block header before it is in storage should return None.
    assert!(get_block_header_by_height(&mut harness, &mut storage, height).is_none());

    let was_new = put_complete_block(&mut harness, &mut storage, Arc::new(block));
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
    let first_block = Block::random(&mut harness.rng);
    put_complete_block(&mut harness, &mut storage, Arc::new(first_block.clone()));
    let second_block = loop {
        // We need to make sure that the second random block has different height than the first
        // one.
        let block = Block::random(&mut harness.rng);
        if block.height() != first_block.height() {
            break block;
        }
    };
    put_complete_block(&mut harness, &mut storage, Arc::new(second_block));
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
    put_complete_block(&mut harness, &mut storage, Arc::new(first_block));
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

    let block_1 = Block::random(&mut harness.rng);
    let fs_1_1 =
        FinalitySignature::random_for_block(*block_1.hash(), block_1.header().era_id().into());
    let fs_1_2 =
        FinalitySignature::random_for_block(*block_1.hash(), block_1.header().era_id().into());

    let block_2 = Block::random(&mut harness.rng);
    let fs_2_1 =
        FinalitySignature::random_for_block(*block_2.hash(), block_2.header().era_id().into());
    let fs_2_2 =
        FinalitySignature::random_for_block(*block_2.hash(), block_2.header().era_id().into());

    let block_3 = Block::random(&mut harness.rng);
    let fs_3_1 =
        FinalitySignature::random_for_block(*block_3.hash(), block_3.header().era_id().into());
    let fs_3_2 =
        FinalitySignature::random_for_block(*block_3.hash(), block_3.header().era_id().into());

    let block_4 = Block::random(&mut harness.rng);

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
    let _ = initialize_block_metadata_db(&storage.env, &storage.block_metadata_db, &to_be_purged);
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
    let to_be_purged = HashSet::from_iter([block_1.hash().as_ref()]);
    let _ = initialize_block_metadata_db(&storage.env, &storage.block_metadata_db, &to_be_purged);
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
    let to_be_purged = HashSet::from_iter([block_4.hash().as_ref()]);
    let _ = initialize_block_metadata_db(&storage.env, &storage.block_metadata_db, &to_be_purged);
    assert_signatures(&storage, *block_1.hash(), vec![]);
    assert_signatures(&storage, *block_2.hash(), vec![fs_2_1, fs_2_2]);
    assert_signatures(&storage, *block_3.hash(), vec![fs_3_1, fs_3_2]);
    assert_signatures(&storage, *block_4.hash(), vec![]);

    // Purging for all blocks should leave no signatures.
    let to_be_purged = HashSet::from_iter([
        block_1.hash().as_ref(),
        block_2.hash().as_ref(),
        block_3.hash().as_ref(),
        block_4.hash().as_ref(),
    ]);

    let _ = initialize_block_metadata_db(&storage.env, &storage.block_metadata_db, &to_be_purged);
    assert_signatures(&storage, *block_1.hash(), vec![]);
    assert_signatures(&storage, *block_2.hash(), vec![]);
    assert_signatures(&storage, *block_3.hash(), vec![]);
    assert_signatures(&storage, *block_4.hash(), vec![]);
}
