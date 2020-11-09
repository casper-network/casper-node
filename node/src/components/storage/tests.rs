use super::{Config, Storage};
use crate::{
    crypto::hash::Digest,
    effect::requests::StorageRequest,
    testing::ComponentHarness,
    types::{Block, BlockHash},
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

#[test]
fn get_block_of_non_existant_block_returns_none() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let block_hash = BlockHash::new(Digest::random(&mut harness.rng));
    let response = harness.send_request(&mut storage, move |responder| {
        StorageRequest::GetBlock {
            block_hash,
            responder,
        }
        .into()
    });

    assert!(response.is_none());
}

#[test]
fn can_put_and_get_block() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    // Create a random block, load and store it.
    let block = Box::new(Block::random(&mut harness.rng));
    let block2 = block.clone();

    let was_new = harness.send_request(&mut storage, move |responder| {
        StorageRequest::PutBlock {
            block: block,
            responder,
        }
        .into()
    });

    assert!(was_new, "putting block should have returned `true`");

    // Storing the same block again should work, but yield a result of `false`.
    let was_new_second_time = harness.send_request(&mut storage, move |responder| {
        StorageRequest::PutBlock {
            block: block2,
            responder,
        }
        .into()
    });

    assert!(
        !was_new_second_time,
        "storing block the second time should have returned `false`"
    );
}

// fn STORAGE(s: StorageRequest) {
//     match s {
//         StorageRequest::GetBlockAtHeight { height, responder } => {}
//         StorageRequest::GetHighestBlock { responder } => {}
//         StorageRequest::GetBlockHeader { block_hash, responder } => {}
//         StorageRequest::PutDeploy { deploy, responder } => {}
//         StorageRequest::GetDeploys { deploy_hashes, responder } => {}
//         StorageRequest::GetDeployHeaders { deploy_hashes, responder } => {}
//         StorageRequest::PutExecutionResults { block_hash, execution_results, responder } => {}
//         StorageRequest::GetDeployAndMetadata { deploy_hash, responder } => {}
//         StorageRequest::PutChainspec { chainspec, responder } => {}
//         StorageRequest::GetChainspec { version, responder } => {}
//     }
// }
