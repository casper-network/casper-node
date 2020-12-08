use casper_execution_engine::{
    core::engine_state::executable_deploy_item::ExecutableDeployItem, shared::motes::Motes,
};
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    runtime_args, RuntimeArgs, U512,
};

use crate::{
    crypto::asymmetric_key::SecretKey,
    testing::TestRng,
    types::{BlockLike, Deploy, DeployHash, DeployHeader, TimeDiff},
};

use super::*;
use casper_types::standard_payment::ARG_AMOUNT;

fn generate_transfer(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
    dependencies: Vec<DeployHash>,
    gas_price: u64,
    payment_amount: U512,
) -> (DeployHash, DeployHeader) {
    let secret_key = SecretKey::random(rng);
    let chain_name = "chain".to_string();
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: Bytes::new(),
    };

    let args = runtime_args! {
        ARG_AMOUNT => payment_amount
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
    gas_price: u64,
) -> (DeployHash, DeployHeader) {
    let secret_key = SecretKey::random(rng);
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

    let no_deploys = HashSet::new();
    let mut proposer = create_test_proposer();
    let mut rng = crate::new_rng();
    let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![], 10);
    let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![], 10);
    let (hash3, deploy3) = generate_deploy(&mut rng, creation_time, ttl, vec![], 10);
    let (hash4, deploy4) = generate_deploy(&mut rng, creation_time, ttl, vec![], 10);

    assert!(proposer
        .propose_proto_block(
            DeployConfig::default(),
            block_time2,
            no_deploys.clone(),
            true
        )
        .deploys()
        .is_empty());

    // add two deploys
    proposer.add_deploy_or_transfer(block_time2, hash1, DeployType::Wasm(deploy1));
    proposer.add_deploy_or_transfer(block_time2, hash2, DeployType::Wasm(deploy2));

    // if we try to create a block with a timestamp that is too early, we shouldn't get any
    // deploys
    assert!(proposer
        .propose_proto_block(
            DeployConfig::default(),
            block_time1,
            no_deploys.clone(),
            true
        )
        .deploys()
        .is_empty());

    // if we try to create a block with a timestamp that is too late, we shouldn't get any
    // deploys, either
    assert!(proposer
        .propose_proto_block(
            DeployConfig::default(),
            block_time3,
            no_deploys.clone(),
            true
        )
        .deploys()
        .is_empty());

    // take the deploys out
    let block = proposer.propose_proto_block(
        DeployConfig::default(),
        block_time2,
        no_deploys.clone(),
        true,
    );
    let deploys = block.deploys();

    assert_eq!(deploys.len(), 2);
    assert!(deploys.contains(&&hash1));
    assert!(deploys.contains(&&hash2));

    // take the deploys out
    let block = proposer.propose_proto_block(
        DeployConfig::default(),
        block_time2,
        no_deploys.clone(),
        true,
    );
    let deploys = block
        .deploys()
        .iter()
        .map(|hash| **hash)
        .collect::<HashSet<_>>();
    assert_eq!(deploys.len(), 2);

    // but they shouldn't be returned if we include it in the past deploys
    assert!(proposer
        .propose_proto_block(DeployConfig::default(), block_time2, deploys.clone(), true)
        .deploys()
        .is_empty());

    // finalize the block
    proposer.finalized_deploys(deploys.iter().copied());

    // add more deploys
    proposer.add_deploy_or_transfer(block_time2, hash3, DeployType::Wasm(deploy3));
    proposer.add_deploy_or_transfer(block_time2, hash4, DeployType::Wasm(deploy4));

    let block =
        proposer.propose_proto_block(DeployConfig::default(), block_time2, no_deploys, true);
    let deploys = block.deploys();

    // since block 1 is now finalized, neither deploy1 nor deploy2 should be among the returned
    assert_eq!(deploys.len(), 2);
    assert!(deploys.contains(&&hash3));
    assert!(deploys.contains(&&hash4));
}

#[test]
fn should_successfully_prune() {
    let expired_time = Timestamp::from(201);
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(100);

    let mut rng = crate::new_rng();
    let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![], 10);
    let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![], 10);
    let (hash3, deploy3) = generate_deploy(&mut rng, creation_time, ttl, vec![], 10);
    let (hash4, deploy4) = generate_deploy(
        &mut rng,
        creation_time + Duration::from_secs(20).into(),
        ttl,
        vec![],
        10,
    );
    let mut proposer = create_test_proposer();

    // pending
    proposer.add_deploy_or_transfer(creation_time, hash1, DeployType::Wasm(deploy1));
    proposer.add_deploy_or_transfer(creation_time, hash2, DeployType::Wasm(deploy2));
    proposer.add_deploy_or_transfer(creation_time, hash3, DeployType::Wasm(deploy3));
    proposer.add_deploy_or_transfer(creation_time, hash4, DeployType::Wasm(deploy4));

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
fn should_respect_limits_for_wasmless_transfers() {
    test_proposer_with(TestArgs {
        transfer_count: 30,
        max_transfer_count: 20,
        proposed_count: 20,
        remaining_pending_count: 10,
        ..Default::default()
    });
}

#[test]
fn should_respect_limits_for_wasm_deploys() {
    test_proposer_with(TestArgs {
        wasm_deploy_count: 30,
        max_deploy_count: 20,
        proposed_count: 20,
        remaining_pending_count: 10,
        ..Default::default()
    });
}

#[test]
fn should_respect_limits_for_wasm_deploys_and_transfers_together() {
    test_proposer_with(TestArgs {
        transfer_count: 30,
        max_transfer_count: 20,
        wasm_deploy_count: 30,
        max_deploy_count: 20,
        proposed_count: 40,
        remaining_pending_count: 20,
        ..Default::default()
    });
}

#[test]
fn should_respect_limits_for_gas_cost() {
    test_proposer_with(TestArgs {
        transfer_count: 30,
        max_transfer_count: 20,
        wasm_deploy_count: 30,
        max_deploy_count: 20,
        gas_cost: 5,
        max_gas_limit: 10,
        proposed_count: 2,
        remaining_pending_count: 58,
        ..Default::default()
    });
}

#[test]
fn should_respect_max_payment_amount_for_transfers() {
    test_proposer_with(TestArgs {
        wasm_deploy_count: 0,
        transfer_count: 1,
        payment_amount: U512::from(100_000),
        max_payment_cost: U512::from(100_000),
        max_transfer_count: 5,
        proposed_count: 1,
        remaining_pending_count: 0,
        ..Default::default()
    });
}

#[test]
fn should_not_propose_transfers_with_payment_amount_above_max_payment_cost() {
    test_proposer_with(TestArgs {
        wasm_deploy_count: 0,
        transfer_count: 1,
        payment_amount: U512::from(100_001),
        max_payment_cost: U512::from(100_000),
        max_transfer_count: 5,
        proposed_count: 0,
        remaining_pending_count: 1,
        ..Default::default()
    });
}

#[derive(Default)]
struct TestArgs {
    /// Number of deploys to create.
    wasm_deploy_count: u32,
    /// Max deploys to propose.
    max_deploy_count: u32,
    /// Number of transfer deploys to create.
    transfer_count: u32,
    /// Max number of transfers to propose.
    max_transfer_count: u32,
    /// Cost for deploys generated.
    gas_cost: u64,
    /// Max gas cost for block.
    max_gas_limit: u64,
    /// Payment amount for transfers.
    payment_amount: U512,
    /// Payment amount for transfers.
    max_payment_cost: U512,
    /// Post-finalization of proposed block, how many transfers and deploys remain.
    remaining_pending_count: usize,
    /// Block deploy count proposed.
    proposed_count: usize,
}


/// Test the block_proposer by generating deploys and transfers with variable limits, asserting
/// on internal counts post-finalization.
fn test_proposer_with(
    TestArgs {
        wasm_deploy_count,
        max_deploy_count,
        transfer_count,
        max_transfer_count,
        gas_cost,
        max_gas_limit,
        payment_amount,
        max_payment_cost,
        remaining_pending_count,
        proposed_count,
    }: TestArgs,
) -> BlockProposerReady {
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(100);
    let past_deploys = HashSet::new();

    let mut rng = crate::new_rng();
    let mut proposer = create_test_proposer();
    let mut config = proposer.deploy_config;
    // defaults are 10, 1000 respectively
    config.block_max_deploy_count = max_deploy_count;
    config.block_max_transfer_count = max_transfer_count;
    config.block_gas_limit = max_gas_limit;
    config.max_payment_cost = Motes::new(max_payment_cost);

    for _ in 0..wasm_deploy_count {
        let (hash, deploy) = generate_deploy(&mut rng, creation_time, ttl, vec![], gas_cost);
        proposer.add_deploy_or_transfer(creation_time, hash, DeployType::Wasm(deploy));
    }
    for _ in 0..transfer_count {
        let (hash, transfer) = generate_transfer(
            &mut rng,
            creation_time,
            ttl,
            vec![],
            gas_cost,
            payment_amount,
        );
        proposer.add_deploy_or_transfer(
            creation_time,
            hash,
            DeployType::Transfer {
                header: transfer,
                payment_amount: Motes::new(payment_amount),
            },
        );
    }

    let block = proposer.propose_proto_block(config, test_time, past_deploys, true);
    let all_deploys = BlockLike::deploys(&block);
    proposer.finalized_deploys(all_deploys.iter().map(|hash| **hash));
    assert_eq!(
        all_deploys.len(),
        proposed_count,
        "should have a proposed_count of {}, but got {}",
        proposed_count,
        all_deploys.len()
    );
    assert_eq!(
        proposer.sets.pending.len(),
        remaining_pending_count,
        "should have a remaining_pending_count of {}, but got {}",
        remaining_pending_count,
        proposer.sets.pending.len()
    );
    proposer
}

#[test]
fn should_return_deploy_dependencies() {
    let creation_time = Timestamp::from(100);
    let ttl = TimeDiff::from(100);
    let block_time = Timestamp::from(120);

    let mut rng = crate::new_rng();
    let (hash1, deploy1) = generate_deploy(&mut rng, creation_time, ttl, vec![], 10);
    // let deploy2 depend on deploy1
    let (hash2, deploy2) = generate_deploy(&mut rng, creation_time, ttl, vec![hash1], 10);

    let no_deploys = HashSet::new();
    let mut proposer = create_test_proposer();

    // add deploy2
    proposer.add_deploy_or_transfer(creation_time, hash2, DeployType::Wasm(deploy2));

    // deploy2 has an unsatisfied dependency
    assert!(proposer
        .propose_proto_block(
            DeployConfig::default(),
            block_time,
            no_deploys.clone(),
            true
        )
        .deploys()
        .is_empty());

    // add deploy1
    proposer.add_deploy_or_transfer(creation_time, hash1, DeployType::Wasm(deploy1));

    let block = proposer.propose_proto_block(
        DeployConfig::default(),
        block_time,
        no_deploys.clone(),
        true,
    );
    let deploys = block
        .deploys()
        .iter()
        .map(|hash| **hash)
        .collect::<Vec<_>>();
    // only deploy1 should be returned, as it has no dependencies
    assert_eq!(deploys.len(), 1);
    assert!(deploys.contains(&hash1));

    // the deploy will be included in block 1
    proposer.finalized_deploys(deploys.iter().copied());

    let block = proposer.propose_proto_block(DeployConfig::default(), block_time, no_deploys, true);
    // `blocks` contains a block that contains deploy1 now, so we should get deploy2
    let deploys2 = block.wasm_deploys();
    assert_eq!(deploys2.len(), 1);
    assert!(deploys2.contains(&hash2));
}
