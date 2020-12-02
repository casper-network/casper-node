use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    runtime_args, RuntimeArgs,
};

use crate::{
    crypto::asymmetric_key::SecretKey,
    testing::TestRng,
    types::{BlockLike, Deploy, DeployHash, DeployHeader, TimeDiff},
};

use super::*;

fn generate_transfer(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
    dependencies: Vec<DeployHash>,
) -> (DeployHash, DeployHeader) {
    let secret_key = SecretKey::random(rng);
    let gas_price = 10;
    let chain_name = "chain".to_string();
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: Bytes::new(),
    };

    let args = runtime_args! {
        "amount" => 42
    }
    .to_bytes()
    .expect("should serialize");
    let session = ExecutableDeployItem::Transfer { args: args.into() };

    let deploy = Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
        rng,
    );

    (*deploy.id(), deploy.take_header())
}

fn generate_deploy(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
    dependencies: Vec<DeployHash>,
) -> (DeployHash, DeployHeader) {
    let secret_key = SecretKey::random(rng);
    let gas_price = 10;
    let chain_name = "chain".to_string();
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: Bytes::new(),
    };
    let session = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: Bytes::new(),
    };

    let deploy = Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
        rng,
    );

    (*deploy.id(), deploy.take_header())
}

fn create_test_proposer() -> BlockProposerReady {
    BlockProposerReady {
        sets: Default::default(),
        deploy_config: Default::default(),
        state_key: b"block-proposer-test".to_vec(),
        request_queue: Default::default(),
    }
}

impl From<StorageRequest> for Event {
    fn from(_: StorageRequest) -> Self {
        // we never send a storage request in our unit tests, but if this does become
        // meaningful....
        unreachable!("no storage requests in block proposer unit tests")
    }
}

impl From<StateStoreRequest> for Event {
    fn from(_: StateStoreRequest) -> Self {
        unreachable!("no state store requests in block proposer unit tests")
    }
}

#[test]
fn should_add_and_take_deploys() {
    let creation_time = Timestamp::from(100);
    let ttl = TimeDiff::from(100);
    let block_time1 = Timestamp::from(80);
    let block_time2 = Timestamp::from(120);
    let block_time3 = Timestamp::from(220);

    let no_deploys = Vec::new();
    let mut proposer = create_test_proposer();
    let mut rng = crate::new_rng();
    let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
    let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
    let (hash3, deploy3) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
    let (hash4, deploy4) = generate_deploy(&mut rng, creation_time, ttl, vec![]);

    assert!(proposer
        .propose_proto_block(DeployConfig::default(), block_time2, &no_deploys, true)
        .deploys()
        .is_empty());

    // add two deploys
    proposer.add_deploy_or_transfer(block_time2, hash1, DeployType::Deploy(deploy1));
    proposer.add_deploy_or_transfer(block_time2, hash2, DeployType::Deploy(deploy2));

    // if we try to create a block with a timestamp that is too early, we shouldn't get any
    // deploys
    assert!(proposer
        .propose_proto_block(DeployConfig::default(), block_time1, &no_deploys, true)
        .deploys()
        .is_empty());

    // if we try to create a block with a timestamp that is too late, we shouldn't get any
    // deploys, either
    assert!(proposer
        .propose_proto_block(DeployConfig::default(), block_time3, &no_deploys, true)
        .deploys()
        .is_empty());

    // take the deploys out
    let block =
        proposer.propose_proto_block(DeployConfig::default(), block_time2, &no_deploys, true);
    let deploys = block.deploys();

    assert_eq!(deploys.len(), 2);
    assert!(deploys.contains(&hash1));
    assert!(deploys.contains(&hash2));

    // take the deploys out
    let block =
        proposer.propose_proto_block(DeployConfig::default(), block_time2, &no_deploys, true);
    let deploys = block.deploys();
    assert_eq!(deploys.len(), 2);

    // but they shouldn't be returned if we include it in the past deploys
    assert!(proposer
        .propose_proto_block(DeployConfig::default(), block_time2, deploys, true)
        .deploys()
        .is_empty());

    // finalize the block
    proposer.finalized_deploys(deploys.to_vec());

    // add more deploys
    proposer.add_deploy_or_transfer(block_time2, hash3, DeployType::Deploy(deploy3));
    proposer.add_deploy_or_transfer(block_time2, hash4, DeployType::Deploy(deploy4));

    let block =
        proposer.propose_proto_block(DeployConfig::default(), block_time2, &no_deploys, true);
    let deploys = block.deploys();

    // since block 1 is now finalized, neither deploy1 nor deploy2 should be among the returned
    assert_eq!(deploys.len(), 2);
    assert!(deploys.contains(&hash3));
    assert!(deploys.contains(&hash4));
}

#[test]
fn should_successfully_prune() {
    let expired_time = Timestamp::from(201);
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(100);

    let mut rng = crate::new_rng();
    let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
    let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
    let (hash3, deploy3) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
    let (hash4, deploy4) = generate_deploy(
        &mut rng,
        creation_time + Duration::from_secs(20).into(),
        ttl,
        vec![],
    );
    let mut proposer = create_test_proposer();

    // pending
    proposer.add_deploy_or_transfer(creation_time, hash1, DeployType::Deploy(deploy1));
    proposer.add_deploy_or_transfer(creation_time, hash2, DeployType::Deploy(deploy2));
    proposer.add_deploy_or_transfer(creation_time, hash3, DeployType::Deploy(deploy3));
    proposer.add_deploy_or_transfer(creation_time, hash4, DeployType::Deploy(deploy4));

    // pending => finalized
    proposer.finalized_deploys(vec![hash1]);

    assert_eq!(proposer.sets.pending.len(), 3);
    assert!(proposer.sets.finalized_deploys.contains_key(&hash1));

    // test for retained values
    let pruned = proposer.prune(test_time);
    assert_eq!(pruned, 0);

    assert_eq!(proposer.sets.pending.len(), 3);
    assert_eq!(proposer.sets.finalized_deploys.len(), 1);
    assert!(proposer.sets.finalized_deploys.contains_key(&hash1));

    // now move the clock to make some things expire
    let pruned = proposer.prune(expired_time);
    assert_eq!(pruned, 3);

    assert_eq!(proposer.sets.pending.len(), 1); // deploy4 is still valid
    assert_eq!(proposer.sets.finalized_deploys.len(), 0);
}

#[test]
fn should_propose_protoblock_with_generated_deploys_and_transfers() {
    // Happy path, less than max limits created
    test_proposer_with(5, 10, 15, 20, 0);
}

#[test]
fn should_not_propose_any_deploys_when_max_values_too_low() {
    // proposer wont propose anything if max is too low
    test_proposer_with(5, 0, 5, 0, 10);
}

#[test]
fn should_propose_protoblock_respecting_limits_for_wasmless_transfers() {
    // proposer should respected limits
    test_proposer_with(10, 5, 35, 20, 20);
}

/// Test the block_proposer by generating deploys and transfers with variable limits, asserting
/// on internal counts post-finalization.
fn test_proposer_with(
    // Number of deploys to create.
    deploys: u32,
    // Max deploys to propose.
    max_deploys: u32,
    // Number of transfer deploys to create.
    transfers: u32,
    // Max number of transfers to propose.
    max_transfers: u32,
    // Post-finalization of proposed block, how many transfers and deploys remain.
    remaining_pending: usize,
) {
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(100);
    let past_deploys = vec![];

    let mut rng = crate::new_rng();
    let mut proposer = create_test_proposer();
    let mut config = proposer.deploy_config;
    // defaults are 10, 1000 respectively
    config.block_max_deploy_count = max_deploys;
    config.block_max_transfer_count = max_transfers;

    for _ in 0..deploys {
        let (hash, deploy) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
        proposer.add_deploy_or_transfer(creation_time, hash, DeployType::Deploy(deploy));
    }
    for _ in 0..transfers {
        let (hash, transfer) = generate_transfer(&mut rng, creation_time, ttl, vec![]);
        proposer.add_deploy_or_transfer(creation_time, hash, DeployType::Transfer(transfer));
    }

    assert_eq!(proposer.sets.pending.len() as u32, deploys + transfers);

    // pending => finalized
    let block = proposer.propose_proto_block(config, test_time, &past_deploys, true);

    assert_eq!(block.deploys().len(), deploys.min(max_deploys) as usize);
    assert_eq!(
        block.transfers().len(),
        transfers.min(max_transfers) as usize
    );

    let all_deploys = BlockLike::deploys(&block);
    assert_eq!(
        all_deploys.len() as u32,
        deploys.min(max_deploys) + transfers.min(max_transfers)
    );

    proposer.finalized_deploys(all_deploys.iter().map(|hash| **hash));
    assert_eq!(proposer.sets.pending.len(), remaining_pending);
}

#[test]
fn should_return_deploy_dependencies() {
    let creation_time = Timestamp::from(100);
    let ttl = TimeDiff::from(100);
    let block_time = Timestamp::from(120);

    let mut rng = crate::new_rng();
    let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![]);
    // let deploy2 depend on deploy1
    let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![hash1]);

    let no_deploys = Vec::new();
    let mut proposer = create_test_proposer();

    // add deploy2
    proposer.add_deploy_or_transfer(creation_time, hash2, DeployType::Deploy(deploy2));

    // deploy2 has an unsatisfied dependency
    assert!(proposer
        .propose_proto_block(DeployConfig::default(), block_time, &no_deploys, true)
        .deploys()
        .is_empty());

    // add deploy1
    proposer.add_deploy_or_transfer(creation_time, hash1, DeployType::Deploy(deploy1));

    let block =
        proposer.propose_proto_block(DeployConfig::default(), block_time, &no_deploys, true);
    let deploys = block.deploys();
    // only deploy1 should be returned, as it has no dependencies
    assert_eq!(deploys.len(), 1);
    assert!(deploys.contains(&hash1));

    // the deploy will be included in block 1
    proposer.finalized_deploys(deploys.iter().copied());

    let block =
        proposer.propose_proto_block(DeployConfig::default(), block_time, &no_deploys, true);
    // `blocks` contains a block that contains deploy1 now, so we should get deploy2
    let deploys2 = block.deploys();
    assert_eq!(deploys2.len(), 1);
    assert!(deploys2.contains(&hash2));
}
