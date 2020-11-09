use smallvec::smallvec;

use super::{Config, Storage};
use crate::{
    crypto::hash::Digest,
    effect::{requests::StorageRequest, Multiple},
    testing::{ComponentHarness, TestRng},
    types::{Block, BlockHash, Deploy, DeployHash},
    utils::WithDir,
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

/// Creates a random block with a specific era.
fn random_block_at_height(rng: &mut TestRng, height: u64) -> Box<Block> {
    let mut block = Box::new(Block::random(rng));
    block.set_height(height);
    block
}

/// Requests block at a specific era.
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

/// Loads a block.
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

/// Loads a deploy.
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

/// Requests the highest block.
fn highest(harness: &mut ComponentHarness<()>, storage: &mut Storage) -> Option<Block> {
    let response = harness.send_request(storage, |responder| {
        StorageRequest::GetHighestBlock { responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores a block.
fn put_block(harness: &mut ComponentHarness<()>, storage: &mut Storage, block: Box<Block>) -> bool {
    let response = harness.send_request(storage, move |responder| {
        StorageRequest::PutBlock { block, responder }.into()
    });
    assert!(harness.is_idle());
    response
}

/// Stores a deploy.
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

    let block_hash = BlockHash::new(Digest::random(&mut harness.rng));
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
fn get_block_of_non_existing_deploy_returns_nones() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let deploy_id = DeployHash::new(Digest::random(&mut harness.rng));
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
}

// fn STORAGE(s: StorageRequest) {
//     match s {
//         StorageRequest::PutExecutionResults { block_hash, execution_results, responder } => {}
//         StorageRequest::GetDeployAndMetadata { deploy_hash, responder } => {}
//         StorageRequest::PutChainspec { chainspec, responder } => {}
//         StorageRequest::GetChainspec { version, responder } => {}
//     }
// }
