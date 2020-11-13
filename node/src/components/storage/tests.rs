//! Unit tests for the storage component.

use std::sync::Arc;

use rand::prelude::SliceRandom;
use semver::Version;
use smallvec::smallvec;

use super::{Config, Storage};
use crate::{
    effect::{requests::StorageRequest, Multiple},
    testing::{ComponentHarness, TestRng},
    types::{
        json_compatibility::ExecutionResult, Block, BlockHash, Deploy, DeployHash, DeployMetadata,
    },
    utils::WithDir,
    Chainspec,
};

/// Storage component test fixture.
///
/// Creates a storage component in a temporary directory.
///
/// # Panics
///
/// Panics if setting up the storage fixture fails.
fn storage_fixture(harness: &mut ComponentHarness<()>) -> Storage {
    let cfg = Config {
        path: harness.tmp.path().join("storage"),
        ..Default::default()
    };

    Storage::new(&WithDir::new(harness.tmp.path(), cfg)).expect(
        "could not create storage component
    fixture",
    )
}

/// Creates a random block with a specific block height.
fn random_block_at_height(rng: &mut TestRng, height: u64) -> Box<Block> {
    let mut block = Box::new(Block::random(rng));
    block.set_height(height);
    block
}

/// Requests block at a specific era from a storage component.
fn at_era(harness: &mut ComponentHarness<()>, storage: &mut Storage, era_id: u64) -> Option<Block> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetBlockAtHeight {
            height: era_id,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

/// Loads a block from a storage component.
fn get_block(
    harness: &mut ComponentHarness<()>,
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

/// Loads the chainspec from a storage component.
fn get_chainspec(
    harness: &mut ComponentHarness<()>,
    storage: &mut Storage,
    version: Version,
) -> Option<Arc<Chainspec>> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetChainspec { version, responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Loads a set of deploys from a storage component.
fn get_deploys(
    harness: &mut ComponentHarness<()>,
    storage: &mut Storage,
    deploy_hashes: Multiple<DeployHash>,
) -> Vec<Option<Deploy>> {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::GetDeploys {
            deploy_hashes,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

/// Loads a deploy with associated metadata from the storage component.
fn get_deploy_and_metadata(
    harness: &mut ComponentHarness<()>,
    storage: &mut Storage,
    deploy_hash: DeployHash,
) -> Option<(Deploy, DeployMetadata)> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetDeployAndMetadata {
            deploy_hash,
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
    response
}

/// Requests the highest block from a storage component.
fn highest(harness: &mut ComponentHarness<()>, storage: &mut Storage) -> Option<Block> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetHighestBlock { responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores a block on a storage component.
fn put_block(harness: &mut ComponentHarness<()>, storage: &mut Storage, block: Box<Block>) -> bool {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutBlock { block, responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores the chainspec on a storage component.
fn put_chainspec(harness: &mut ComponentHarness<()>, storage: &mut Storage, chainspec: Chainspec) {
    harness.send_request(storage, move |responder| {
        StorageRequest::PutChainspec {
            chainspec: Arc::new(chainspec),
            responder,
        }
        .into()
    });
    assert!(harness.is_idle());
}

/// Stores a deploy on the storage component.
fn put_deploy(
    harness: &mut ComponentHarness<()>,
    storage: &mut Storage,
    deploy: Box<Deploy>,
) -> bool {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutDeploy { deploy, responder }.into()
    });
    assert!(harness.is_idle());
    response
}

#[test]
fn get_block_of_non_existing_block_returns_none() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let block_hash = BlockHash::random(&mut harness.rng);
    let response = get_block(&mut harness, &mut storage, block_hash);

    assert!(response.is_none());
    assert!(harness.is_idle());
}

#[test]
fn can_put_and_get_block() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    // Create a random block, store and load it.
    let block = Box::new(Block::random(&mut harness.rng));

    let was_new = put_block(&mut harness, &mut storage, block.clone());
    assert!(was_new, "putting block should have returned `true`");

    // Storing the same block again should work, but yield a result of `false`.
    let was_new_second_time = put_block(&mut harness, &mut storage, block.clone());
    assert!(
        !was_new_second_time,
        "storing block the second time should have returned `false`"
    );

    let response = get_block(&mut harness, &mut storage, block.hash().clone());
    assert_eq!(response.as_ref(), Some(&*block));

    // Also ensure we can retrieve just the header.
    let response = harness.send_request(&mut storage, |responder| {
        StorageRequest::GetBlockHeader {
            block_hash: block.hash().clone(),
            responder,
        }
        .into()
    });

    assert_eq!(response.as_ref(), Some(block.header()));
}

#[test]
fn can_retrieve_block_by_era_id() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    // Create a random block, load and store it.
    let block_33_a = random_block_at_height(&mut harness.rng, 33);
    let block_33_b = random_block_at_height(&mut harness.rng, 33);
    let block_14 = random_block_at_height(&mut harness.rng, 14);
    let block_99 = random_block_at_height(&mut harness.rng, 99);

    // Both block at ID and highest block should return `None` initially.
    assert!(at_era(&mut harness, &mut storage, 0).is_none());
    assert!(highest(&mut harness, &mut storage).is_none());
    assert!(at_era(&mut harness, &mut storage, 14).is_none());
    assert!(at_era(&mut harness, &mut storage, 33).is_none());
    assert!(at_era(&mut harness, &mut storage, 99).is_none());

    // Inserting 33A changes this.
    let was_new = put_block(&mut harness, &mut storage, block_33_a.clone());
    assert!(was_new);

    assert_eq!(
        highest(&mut harness, &mut storage).as_ref(),
        Some(&*block_33_a)
    );
    assert!(at_era(&mut harness, &mut storage, 0).is_none());
    assert!(at_era(&mut harness, &mut storage, 14).is_none());
    assert_eq!(
        at_era(&mut harness, &mut storage, 33).as_ref(),
        Some(&*block_33_a)
    );
    assert!(at_era(&mut harness, &mut storage, 99).is_none());

    // Inserting block with height 14, no change in highest.
    let was_new = put_block(&mut harness, &mut storage, block_14.clone());
    assert!(was_new);

    assert_eq!(
        highest(&mut harness, &mut storage).as_ref(),
        Some(&*block_33_a)
    );
    assert!(at_era(&mut harness, &mut storage, 0).is_none());
    assert_eq!(
        at_era(&mut harness, &mut storage, 14).as_ref(),
        Some(&*block_14)
    );
    assert_eq!(
        at_era(&mut harness, &mut storage, 33).as_ref(),
        Some(&*block_33_a)
    );
    assert!(at_era(&mut harness, &mut storage, 99).is_none());

    // Inserting block with height 99, changes highest.
    let was_new = put_block(&mut harness, &mut storage, block_99.clone());
    assert!(was_new);

    assert_eq!(
        highest(&mut harness, &mut storage).as_ref(),
        Some(&*block_99)
    );
    assert!(at_era(&mut harness, &mut storage, 0).is_none());
    assert_eq!(
        at_era(&mut harness, &mut storage, 14).as_ref(),
        Some(&*block_14)
    );
    assert_eq!(
        at_era(&mut harness, &mut storage, 33).as_ref(),
        Some(&*block_33_a)
    );
    assert_eq!(
        at_era(&mut harness, &mut storage, 99).as_ref(),
        Some(&*block_99)
    );

    // Finally updating 33B should not change highest, but block at height 33.
    let was_new = put_block(&mut harness, &mut storage, block_33_b.clone());
    assert!(was_new);

    assert_eq!(
        highest(&mut harness, &mut storage).as_ref(),
        Some(&*block_99)
    );
    assert!(at_era(&mut harness, &mut storage, 0).is_none());
    assert_eq!(
        at_era(&mut harness, &mut storage, 14).as_ref(),
        Some(&*block_14)
    );
    assert_eq!(
        at_era(&mut harness, &mut storage, 33).as_ref(),
        Some(&*block_33_b)
    );
    assert_eq!(
        at_era(&mut harness, &mut storage, 99).as_ref(),
        Some(&*block_99)
    );
}

#[test]
fn get_vec_of_non_existing_deploy_returns_nones() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let deploy_id = DeployHash::random(&mut harness.rng);
    let response = get_deploys(&mut harness, &mut storage, smallvec![deploy_id]);
    assert_eq!(response, vec![None]);

    // Also verify that we can retrieve using an empty set of deploy hashes.
    let response = get_deploys(&mut harness, &mut storage, smallvec![]);
    assert!(response.is_empty());
}

#[test]
fn can_retrieve_store_and_load_deploys() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    // Create a random deploy, store and load it.
    let deploy = Box::new(Deploy::random(&mut harness.rng));

    let was_new = put_deploy(&mut harness, &mut storage, deploy.clone());
    assert!(was_new, "putting deploy should have returned `true`");

    // Storing the same deploy again should work, but yield a result of `false`.
    let was_new_second_time = put_deploy(&mut harness, &mut storage, deploy.clone());
    assert!(
        !was_new_second_time,
        "storing deploy the second time should have returned `false`"
    );

    // Retrieve the stored deploy.
    let response = get_deploys(&mut harness, &mut storage, smallvec![deploy.id().clone()]);
    assert_eq!(response, vec![Some(deploy.as_ref().clone())]);

    // Also ensure we can retrieve just the header.
    let response = harness.send_request(&mut storage, |responder| {
        StorageRequest::GetDeployHeaders {
            deploy_hashes: smallvec![deploy.id().clone()],
            responder,
        }
        .into()
    });
    assert_eq!(response, vec![Some(deploy.header().clone())]);

    // Finally try to get the metadata as well. Since we did not store any, we expect empty default
    // metadata to present.
    let (deploy_response, metadata_response) = harness
        .send_request(&mut storage, |responder| {
            StorageRequest::GetDeployAndMetadata {
                deploy_hash: deploy.id().clone(),
                responder,
            }
            .into()
        })
        .expect("no deploy with metadata returned");

    assert_eq!(deploy_response, *deploy);
    assert_eq!(metadata_response, DeployMetadata::default());
}

#[test]
fn store_and_load_a_lot_of_deploys() {
    // There is a quite a bit confusion about whether or not `commit()` must be called after
    // read-only transactions
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let total = 1000;
    let batch_size = 25;

    let mut deploy_hashes = Vec::new();

    for _ in 0..total {
        let deploy = Box::new(Deploy::random(&mut harness.rng));
        deploy_hashes.push(deploy.id().clone());
        put_deploy(&mut harness, &mut storage, deploy);
    }

    // Shuffle deploy hashes around to get a random order.
    deploy_hashes.as_mut_slice().shuffle(&mut harness.rng);

    // Retrieve all from storage, ensuring they are found.
    for chunk in deploy_hashes.chunks(batch_size) {
        let result = get_deploys(
            &mut harness,
            &mut storage,
            chunk.into_iter().cloned().collect(),
        );
        assert!(result.iter().all(Option::is_some));
    }
}

#[test]
fn store_random_execution_results() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let block_hash = BlockHash::random(&mut harness.rng);

    let num_fixtures = 1;

    let mut expected = Vec::new();
    for _ in 0..num_fixtures {
        let deploy = Deploy::random(&mut harness.rng);
        let execution_result = ExecutionResult::random(&mut harness.rng);
        expected.push((deploy, execution_result));
    }

    // Create three random execution result values.
    let execution_results = expected
        .iter()
        .map(|(deploy, execution_result)| (deploy.id().clone(), execution_result.clone()))
        .collect();

    let response = harness.send_request(&mut storage, move |responder| {
        StorageRequest::PutExecutionResults {
            block_hash,
            execution_results,
            responder,
        }
        .into()
    });

    // We assume that there are 0 resuls already stored.
    assert_eq!(response, 0);

    // Retrieve all and ensure execution results are the same we put in.
    for (deploy, execution_result) in expected {
        let (actual_deploy, deploy_metadata) =
            get_deploy_and_metadata(&mut harness, &mut storage, deploy.id().clone())
                .expect("deploy and metadata not found, even though they were added");

        // We expect execution results for precisely one block.
        assert_eq!(actual_deploy, deploy);
        assert_eq!(deploy_metadata.execution_results.len(), 1);
        assert_eq!(
            deploy_metadata.execution_results[&block_hash],
            execution_result
        );
    }
}

#[test]
fn store_and_load_chainspec() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let version = Version::new(1, 2, 3);

    // Initially expect a `None` value for the chainspec.
    let response = get_chainspec(&mut harness, &mut storage, version.clone());
    assert!(response.is_none());

    // Store a random chainspec.
    let chainspec = Chainspec::random(&mut harness.rng);
    let response = put_chainspec(&mut harness, &mut storage, chainspec.clone());

    // Compare returned chainspec.
    let response = get_chainspec(&mut harness, &mut storage, version);
    assert_eq!(response, Some(Arc::new(chainspec)));
}

#[test]
fn test_legacy_interface() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let deploy = Box::new(Deploy::random(&mut harness.rng));
    let was_new = put_deploy(&mut harness, &mut storage, deploy.clone());
    assert!(was_new);

    // Ensure we get the deploy we expect.
    let result = storage.handle_legacy_direct_deploy_request(deploy.id().clone());
    assert_eq!(result, Some(*deploy));

    // A non-existant deploy should simply return `None`.
    assert!(storage
        .handle_legacy_direct_deploy_request(DeployHash::random(&mut harness.rng))
        .is_none())
}

// TODO: Test actual persistence on disk happens.
