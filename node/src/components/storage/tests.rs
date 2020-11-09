use super::{Config, Storage};
use crate::{
    crypto::hash::Digest, effect::requests::StorageRequest, testing::ComponentHarness,
    types::BlockHash, utils::WithDir,
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

// #[test]
// fn can_put_and_get_block() {
//     panic!("YAY");
// }

// fn STORAGE(s: StorageRequest) {
//     match s {
//         StorageRequest::PutBlock { block, responder } => {}
//         StorageRequest::GetBock { block_hash, responder } => {}
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
