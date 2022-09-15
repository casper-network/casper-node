//! Unit tests for the storage component.

use std::{
    collections::{BTreeMap, HashMap},
    f32::consts::E,
    fs::{self, File},
    iter,
};

use num_rational::Ratio;
use rand::{prelude::SliceRandom, Rng};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use casper_types::{
    system::auction::UnbondingPurse, testing::TestRng, AccessRights, EraId, ExecutionResult,
    ProtocolVersion, PublicKey, SecretKey, Signature, URef, U512,
};

use super::{
    move_storage_files_to_network_subdir, should_move_storage_files_to_network_subdir, Config,
    Storage,
};
use crate::{
    effect::{requests::StorageRequest, Multiple},
    storage::{
        lmdb_ext::{deserialize_internal, serialize_internal},
        SyncLeapResult,
    },
    testing::{ComponentHarness, UnitTestEvent},
    types::{
        Block, BlockHash, BlockHashAndHeight, BlockHeader, BlockHeaderWithMetadata,
        BlockSignatures, Deploy, DeployHash, DeployMetadata, DeployMetadataExt,
        DeployWithFinalizedApprovals, FetcherItem, FinalitySignature,
    },
    utils::WithDir,
};

const RECENT_ERA_COUNT: u64 = 7;

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

fn create_sync_leap_test_chain(non_signed_blocks: &[u64]) -> (Storage, Vec<Block>) {
    // Test chain:
    //      S0 B1 B2 S3 B4 B5 S6 B7 B8 S9 B10 B11
    //  era 0 | era 1  | era 2  | era 3  | era 4 ...
    //  where
    //   S - switch block
    //   B - non-switch block

    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);
    let validator_1_public_key = {
        let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    };
    let validator_2_public_key = {
        let secret_key = SecretKey::ed25519_from_bytes([4; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    };
    let mut trusted_validator_weights = BTreeMap::new();
    trusted_validator_weights.insert(validator_1_public_key.clone(), U512::from(2000000000000u64));
    trusted_validator_weights.insert(validator_2_public_key.clone(), U512::from(2000000000000u64));

    let mut blocks = vec![];
    [0_u64, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        .iter()
        .for_each(|height| {
            let parent = if *height == 0 {
                None
            } else {
                Some(blocks.get((height - 1) as usize).unwrap())
            };
            let block = Block::random_with_specifics_and_parent_and_validator_weights(
                &mut harness.rng,
                if *height == 0 {
                    EraId::from(0)
                } else {
                    EraId::from((*height - 1) / 3 + 1)
                },
                *height,
                ProtocolVersion::from_parts(1, 5, 0),
                *height == 0 || *height == 3 || *height == 6 || *height == 9,
                None,
                parent,
                if *height == 0 || *height == 3 || *height == 6 || *height == 9 {
                    trusted_validator_weights.clone()
                } else {
                    BTreeMap::new()
                },
            );

            blocks.push(block.clone());
        });
    blocks.iter().for_each(|block| {
        storage.write_block(&block).unwrap();

        let mut proofs = BTreeMap::new();
        proofs.insert(validator_1_public_key.clone(), Signature::System);
        proofs.insert(validator_2_public_key.clone(), Signature::System);

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
    (storage, blocks)
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
        Ratio::new(1, 3),
        None,
        ProtocolVersion::from_parts(1, 0, 0),
        "test",
        RECENT_ERA_COUNT,
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
    let cfg = new_config(harness);
    Storage::new(
        &WithDir::new(harness.tmp.path(), cfg),
        Ratio::new(1, 3),
        Some(reset_era_id),
        ProtocolVersion::from_parts(1, 1, 0),
        "test",
        RECENT_ERA_COUNT,
    )
    .expect("could not create storage component fixture")
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
fn get_block_header_at_height(storage: &mut Storage, height: u64) -> Option<BlockHeader> {
    storage
        .read_block_header_by_height(height)
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
fn get_block_signatures(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block_hash: BlockHash,
) -> Option<BlockSignatures> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetBlockSignatures {
            block_hash,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
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

/// Requests the highest block from a storage component.
fn get_highest_block(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
) -> Option<Block> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetHighestBlock { responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Requests the highest block header from a storage component.
fn get_highest_block_header(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
) -> Option<BlockHeader> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetHighestBlockHeader { responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores a block in a storage component.
fn put_block(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    block: Box<Block>,
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

/// Stores a deploy in a storage component.
fn put_deploy(
    harness: &mut ComponentHarness<UnitTestEvent>,
    storage: &mut Storage,
    deploy: Box<Deploy>,
) -> bool {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutDeploy { deploy, responder }.into()
    });
    assert!(harness.is_idle());
    response
}

fn insert_to_deploy_index(
    storage: &mut Storage,
    deploy: Deploy,
    block_hash_and_height: BlockHashAndHeight,
) -> bool {
    storage
        .deploy_hash_index
        .insert(*deploy.id(), block_hash_and_height)
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
fn can_retrieve_block_by_height() {
    let mut harness = ComponentHarness::default();

    // Create a random blocks, load and store them.
    let block_33 = Box::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        33,
        ProtocolVersion::from_parts(1, 5, 0),
        true,
        None,
    ));
    let block_14 = Box::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        14,
        ProtocolVersion::from_parts(1, 5, 0),
        false,
        None,
    ));
    let block_99 = Box::new(Block::random_with_specifics(
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
    assert!(get_block_header_at_height(&mut storage, 0).is_none());
    assert!(get_highest_block(&mut harness, &mut storage).is_none());
    assert!(get_highest_block_header(&mut harness, &mut storage).is_none());
    assert!(get_block_at_height(&mut storage, 14).is_none());
    assert!(get_block_header_at_height(&mut storage, 14).is_none());
    assert!(get_block_at_height(&mut storage, 33).is_none());
    assert!(get_block_header_at_height(&mut storage, 33).is_none());
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99).is_none());

    // Inserting 33 changes this.
    let was_new = put_block(&mut harness, &mut storage, block_33.clone());
    assert!(was_new);

    assert_eq!(
        get_highest_block(&mut harness, &mut storage).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_highest_block_header(&mut harness, &mut storage).as_ref(),
        Some(block_33.header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0).is_none());
    assert!(get_block_at_height(&mut storage, 14).is_none());
    assert!(get_block_header_at_height(&mut storage, 14).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33).as_ref(),
        Some(block_33.header())
    );
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99).is_none());

    // Inserting block with height 14, no change in highest.
    let was_new = put_block(&mut harness, &mut storage, block_14.clone());
    assert!(was_new);

    assert_eq!(
        get_highest_block(&mut harness, &mut storage).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_highest_block_header(&mut harness, &mut storage).as_ref(),
        Some(block_33.header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 14).as_ref(),
        Some(&*block_14)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14).as_ref(),
        Some(block_14.header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33).as_ref(),
        Some(block_33.header())
    );
    assert!(get_block_at_height(&mut storage, 99).is_none());
    assert!(get_block_header_at_height(&mut storage, 99).is_none());

    // Inserting block with height 99, changes highest.
    let was_new = put_block(&mut harness, &mut storage, block_99.clone());
    assert!(was_new);

    assert_eq!(
        get_highest_block(&mut harness, &mut storage).as_ref(),
        Some(&*block_99)
    );
    assert_eq!(
        get_highest_block_header(&mut harness, &mut storage).as_ref(),
        Some(block_99.header())
    );
    assert!(get_block_at_height(&mut storage, 0).is_none());
    assert!(get_block_header_at_height(&mut storage, 0).is_none());
    assert_eq!(
        get_block_at_height(&mut storage, 14).as_ref(),
        Some(&*block_14)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 14).as_ref(),
        Some(block_14.header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 33).as_ref(),
        Some(&*block_33)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 33).as_ref(),
        Some(block_33.header())
    );
    assert_eq!(
        get_block_at_height(&mut storage, 99).as_ref(),
        Some(&*block_99)
    );
    assert_eq!(
        get_block_header_at_height(&mut storage, 99).as_ref(),
        Some(block_99.header())
    );
}

#[test]
#[should_panic(expected = "duplicate entries")]
fn different_block_at_height_is_fatal() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    // Create two different blocks at the same height.
    let block_44_a = Box::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        44,
        ProtocolVersion::V1_0_0,
        false,
        None,
    ));
    let block_44_b = Box::new(Block::random_with_specifics(
        &mut harness.rng,
        EraId::new(1),
        44,
        ProtocolVersion::V1_0_0,
        false,
        None,
    ));

    let was_new = put_block(&mut harness, &mut storage, block_44_a.clone());
    assert!(was_new);

    let was_new = put_block(&mut harness, &mut storage, block_44_a);
    assert!(was_new);

    // Putting a different block with the same height should now crash.
    put_block(&mut harness, &mut storage, block_44_b);
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
    let deploy = Box::new(Deploy::random(&mut harness.rng));

    let was_new = put_deploy(&mut harness, &mut storage, deploy.clone());
    let block_hash_and_height = BlockHashAndHeight::random(&mut harness.rng);
    // Insert to the deploy hash index as well so that we can perform the GET later.
    // Also check that we don't have an entry there for this deploy.
    assert!(insert_to_deploy_index(
        &mut storage,
        *deploy.clone(),
        block_hash_and_height
    ));
    assert!(was_new, "putting deploy should have returned `true`");

    // Storing the same deploy again should work, but yield a result of `false`.
    let was_new_second_time = put_deploy(&mut harness, &mut storage, deploy.clone());
    assert!(
        !was_new_second_time,
        "storing deploy the second time should have returned `false`"
    );
    assert!(!insert_to_deploy_index(
        &mut storage,
        *deploy.clone(),
        block_hash_and_height
    ));

    // Retrieve the stored deploy.
    let response = get_naive_deploys(&mut harness, &mut storage, smallvec![*deploy.id()]);
    assert_eq!(response, vec![Some(deploy.as_ref().clone())]);

    // Finally try to get the metadata as well. Since we did not store any, we expect to get the
    // block hash and height from the indices.
    let (deploy_response, metadata_response) = harness
        .send_request(&mut storage, |responder| {
            StorageRequest::GetDeployAndMetadata {
                deploy_hash: *deploy.id(),
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
            assert_eq!(block_hash_and_height, recv_block_hash_and_height)
        }
        DeployMetadataExt::Empty => panic!(
            "We stored block info in the deploy hash index \
                                            but we received nothing in the response."
        ),
    }

    // Create a random deploy, store and load it.
    let deploy = Box::new(Deploy::random(&mut harness.rng));

    assert!(put_deploy(&mut harness, &mut storage, deploy.clone()));
    // Don't insert to the deploy hash index. Since we have no execution results
    // either, we should receive an empty metadata response.
    let (deploy_response, metadata_response) = harness
        .send_request(&mut storage, |responder| {
            StorageRequest::GetDeployAndMetadata {
                deploy_hash: *deploy.id(),
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
fn storing_and_loading_a_lot_of_deploys_does_not_exhaust_handles() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let total = 1000;
    let batch_size = 25;

    let mut deploy_hashes = Vec::new();

    for _ in 0..total {
        let deploy = Box::new(Deploy::random(&mut harness.rng));
        deploy_hashes.push(*deploy.id());
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
    put_deploy(&mut harness, &mut storage, Box::new(deploy.clone()));

    // Ensure deploy exists.
    assert_eq!(
        get_naive_deploys(&mut harness, &mut storage, smallvec![*deploy.id()]),
        vec![Some(deploy.clone())]
    );

    // Put first execution result.
    let first_result: ExecutionResult = harness.rng.gen();
    let mut first_results = HashMap::new();
    first_results.insert(*deploy.id(), first_result.clone());
    put_execution_results(&mut harness, &mut storage, block_hash_a, first_results);

    // Retrieve and check if correct.
    let (first_deploy, first_metadata) =
        get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy.id())
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
    second_results.insert(*deploy.id(), second_result.clone());
    put_execution_results(&mut harness, &mut storage, block_hash_b, second_results);

    // Retrieve the deploy again, should now contain both.
    let (second_deploy, second_metadata) =
        get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy.id())
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
        put_deploy(&mut harness, &mut storage, Box::new(deploy.clone()));
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
            put_deploy(harness, storage, Box::new(deploy.clone()));

            let execution_result: ExecutionResult = harness.rng.gen();

            // Insert deploy results for the unique block-deploy combination.
            let mut map = HashMap::new();
            map.insert(*block_hash, execution_result.clone());
            expected_outcome.insert(*deploy.id(), map);

            // Add to our expected outcome.
            block_results.insert(*deploy.id(), execution_result);
        }

        // Insert the shared deploys as well.
        for shared_deploy in shared_deploys {
            let execution_result: ExecutionResult = harness.rng.gen();

            // Insert the new result and ensure it is not present yet.
            let result = block_results.insert(*shared_deploy.id(), execution_result.clone());
            assert!(result.is_none());

            // Insert into expected outcome.
            let deploy_expected = expected_outcome.entry(*shared_deploy.id()).or_default();
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

        assert_eq!(deploy_hash, deploy.id());

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

#[test]
fn store_identical_execution_results() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let block_hash = BlockHash::random(&mut harness.rng);
    let deploy_hash = DeployHash::random(&mut harness.rng);

    let mut exec_result = HashMap::new();
    exec_result.insert(deploy_hash, harness.rng.gen());

    put_execution_results(&mut harness, &mut storage, block_hash, exec_result.clone());

    // We should be fine storing the exact same result twice.
    put_execution_results(&mut harness, &mut storage, block_hash, exec_result);
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

    let deploy = Box::new(Deploy::random(&mut harness.rng));
    let was_new = put_deploy(&mut harness, &mut storage, deploy.clone());
    assert!(was_new);

    // Ensure we get the deploy we expect.
    let result = storage.get_deploy(*deploy.id()).expect("should get deploy");
    assert_eq!(result, Some(*deploy));

    // A non-existent deploy should simply return `None`.
    assert!(storage
        .get_deploy(DeployHash::random(&mut harness.rng))
        .expect("should get deploy")
        .is_none())
}

#[test]
fn persist_blocks_deploys_and_deploy_metadata_across_instantiations() {
    let mut harness = ComponentHarness::default();
    let mut storage = storage_fixture(&harness);

    let mut block = Block::random(&mut harness.rng);
    let block_height = block.height();

    // Create some sample data.
    let deploy = Deploy::random(&mut harness.rng);
    let execution_result: ExecutionResult = harness.rng.gen();
    put_deploy(&mut harness, &mut storage, Box::new(deploy.clone()));
    put_block(
        &mut harness,
        &mut storage,
        Box::new(block.disable_switch_block().clone()),
    );
    let mut execution_results = HashMap::new();
    execution_results.insert(*deploy.id(), execution_result.clone());
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
    let actual_deploys = get_naive_deploys(&mut harness, &mut storage, smallvec![*deploy.id()]);
    assert_eq!(actual_deploys, vec![Some(deploy.clone())]);

    let (_, deploy_metadata_ext) =
        get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy.id())
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
        assert!(put_block(
            &mut harness,
            &mut storage,
            Box::new(block.clone())
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
        put_deploy(&mut harness, &mut storage, Box::new(deploy.clone()));
        let mut exec_results = HashMap::new();
        exec_results.insert(*deploy.id(), execution_result);
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
        get_highest_block(&mut harness, &mut storage)
    );

    // The closure doing the actual checks.
    let mut check = |reset_era: usize| {
        // Initialize a new storage with a hard reset to the given era, deleting blocks from that
        // era onwards.
        let mut storage = storage_fixture_with_hard_reset(&harness, EraId::from(reset_era as u64));

        // Check highest block is the last from the previous era, or `None` if resetting to era 0.
        let highest_block = get_highest_block(&mut harness, &mut storage);
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
            let result = get_block_signatures(&mut harness, &mut storage, *block.hash());
            let should_get_sigs = index < blocks_per_era * reset_era;
            assert_eq!(should_get_sigs, result.is_some());
        }

        // Check execution results in deleted blocks have been removed.
        for (index, deploy) in deploys.iter().enumerate() {
            let (_, deploy_metadata_ext) =
                get_naive_deploy_and_metadata(&mut harness, &mut storage, *deploy.id()).unwrap();
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
        Ratio::new(1, 3),
        None,
        ProtocolVersion::from_parts(1, 0, 0),
        network_name,
        RECENT_ERA_COUNT,
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
    let block = Box::new(block);

    let mut storage = storage_fixture(&harness);

    let was_new = put_block(&mut harness, &mut storage, block.clone());
    assert!(was_new, "putting block should have returned `true`");

    // Storing the same block again should work, but yield a result of `true`.
    let was_new_second_time = put_block(&mut harness, &mut storage, block.clone());
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
    let (storage, blocks) = create_sync_leap_test_chain(&[]);

    let get_results = |requested_height: usize| -> Vec<u64> {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let requested_block_header = blocks.get(requested_height).unwrap().header();
        storage
            .get_trusted_ancestor_headers(&mut txn, &requested_block_header)
            .unwrap()
            .unwrap()
            .iter()
            .map(|block_header| block_header.height())
            .collect()
    };

    assert!(get_results(6).is_empty());
    assert_eq!(get_results(8), &[7, 6]);
    assert_eq!(get_results(4), &[3]);
}

#[test]
fn should_get_signed_block_headers() {
    let (storage, blocks) = create_sync_leap_test_chain(&[]);

    let get_results = |requested_height: usize, previous_switch_block: usize| -> Vec<u64> {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let requested_block_header = blocks.get(requested_height).unwrap().header();
        let highest_block_header_with_sufficient_signatures = storage
            .get_header_of_highest_complete_block(&mut txn)
            .unwrap()
            .unwrap();
        let previous_switch_block_header = storage
            .get_block_by_height(&mut txn, previous_switch_block as u64)
            .unwrap()
            .unwrap()
            .header()
            .clone();
        storage
            .get_signed_block_headers(
                &mut txn,
                &requested_block_header,
                &highest_block_header_with_sufficient_signatures,
                previous_switch_block_header.clone(),
            )
            .unwrap()
            .unwrap()
            .iter()
            .map(|block_header_with_metadata| block_header_with_metadata.block_header.height())
            .collect()
    };

    assert!(
        get_results(11, 9).is_empty(),
        "should return empty set if asked for a most recent signed block"
    );
    assert_eq!(get_results(4, 3), &[6, 9, 11]);
    assert_eq!(get_results(1, 0), &[3, 6, 9, 11]);
    assert_eq!(
        get_results(9, 6),
        &[11],
        "should return only tip if asked for a most recent switch block"
    );
    assert_eq!(
        get_results(6, 3),
        &[9, 11],
        "should not include switch block that was directly requested"
    );
}

#[test]
fn should_get_signed_block_headers_when_no_sufficient_finality_in_most_recent_block() {
    let (storage, blocks) = create_sync_leap_test_chain(&[11]);

    let get_results = |requested_height: usize, previous_switch_block: usize| -> Vec<u64> {
        let mut txn = storage.env.begin_ro_txn().unwrap();
        let requested_block_header = blocks.get(requested_height).unwrap().header();
        let highest_block_header_with_sufficient_signatures = storage
            .get_header_of_highest_complete_block(&mut txn)
            .unwrap()
            .unwrap();

        let previous_switch_block_header = storage
            .get_block_by_height(&mut txn, previous_switch_block as u64)
            .unwrap()
            .unwrap()
            .header()
            .clone();
        storage
            .get_signed_block_headers(
                &mut txn,
                &requested_block_header,
                &highest_block_header_with_sufficient_signatures,
                previous_switch_block_header.clone(),
            )
            .unwrap()
            .unwrap()
            .iter()
            .map(|block_header_with_metadata| block_header_with_metadata.block_header.height())
            .collect()
    };

    assert!(
        get_results(10, 9).is_empty(),
        "should return empty set if asked for a most recent signed block"
    );
    assert_eq!(get_results(4, 3), &[6, 9, 10]);
    assert_eq!(get_results(1, 0), &[3, 6, 9, 10]);
    assert_eq!(
        get_results(9, 6),
        &[10],
        "should return only tip if asked for a most recent switch block"
    );
    assert_eq!(
        get_results(6, 3),
        &[9, 10],
        "should not include switch block that was directly requested"
    );
}

#[test]
fn should_get_sync_leap() {
    let (storage, blocks) = create_sync_leap_test_chain(&[]);

    let requested_block_hash = blocks.get(5).unwrap().header().hash();
    let allowed_era_diff = 100;
    let sync_leap_result = storage
        .get_sync_leap(requested_block_hash, allowed_era_diff)
        .unwrap();

    let sync_leap = match sync_leap_result {
        SyncLeapResult::HaveIt(sync_leap) => sync_leap,
        _ => panic!("should have leap sync"),
    };

    assert_eq!(sync_leap.trusted_block_header.height(), 5);
    assert_eq!(
        block_headers_into_heights(&sync_leap.trusted_ancestor_headers),
        vec![4, 3],
    );
    assert_eq!(
        signed_block_headers_into_heights(&sync_leap.signed_block_headers),
        vec![6, 9, 11]
    );

    // TODO: Add correct sync leap validation.
    // assert!(sync_leap.validate(Ration::new(1, 3)));
}

#[test]
fn should_respect_allowed_era_diff_in_get_sync_leap() {
    let (storage, blocks) = create_sync_leap_test_chain(&[]);

    let requested_block_hash = blocks.get(5).unwrap().header().hash();
    let allowed_era_diff = 1;
    let sync_leap_result = storage
        .get_sync_leap(requested_block_hash, allowed_era_diff)
        .unwrap();

    assert!(
        matches!(sync_leap_result, SyncLeapResult::TooOld),
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

    let was_new = put_block(&mut harness, &mut storage, Box::new(block));
    assert!(was_new);

    // Requesting the block header after it is in storage should return the block header.
    let maybe_block_header = get_block_header_by_height(&mut harness, &mut storage, height);
    assert!(maybe_block_header.is_some());
    assert_eq!(expected_header, maybe_block_header.unwrap());
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
