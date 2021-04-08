use casper_execution_engine::{
    core::engine_state::executable_deploy_item::ExecutableDeployItem, shared::gas::Gas,
};
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    runtime_args,
    system::standard_payment::ARG_AMOUNT,
    RuntimeArgs, SecretKey,
};

use super::*;
use crate::{
    crypto::AsymmetricKeyExt,
    testing::TestRng,
    types::{BlockLike, Deploy, DeployHash, TimeDiff},
};

const DEFAULT_TEST_GAS_PRICE: u64 = 1;

fn default_gas_payment() -> Gas {
    Gas::from(1u32)
}

fn generate_transfer(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
    dependencies: Vec<DeployHash>,
    payment_amount: Gas,
) -> Deploy {
    let gas_price = DEFAULT_TEST_GAS_PRICE;
    let secret_key = SecretKey::random(rng);
    let chain_name = "chain".to_string();

    let args = runtime_args! {
        ARG_AMOUNT => payment_amount.value()
    };
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args,
    };

    let session = ExecutableDeployItem::Transfer {
        args: RuntimeArgs::new(),
    };

    Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
    )
}

fn generate_deploy(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
    dependencies: Vec<DeployHash>,
    payment_amount: Gas,
    gas_price: u64,
) -> Deploy {
    let secret_key = SecretKey::random(rng);
    let chain_name = "chain".to_string();
    let args = runtime_args! {
        ARG_AMOUNT => payment_amount.value()
    };
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args,
    };
    let session = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: RuntimeArgs::new(),
    };

    Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
    )
}

fn create_test_proposer(deploy_delay: TimeDiff) -> BlockProposerReady {
    BlockProposerReady {
        sets: Default::default(),
        deploy_config: Default::default(),
        state_key: b"block-proposer-test".to_vec(),
        request_queue: Default::default(),
        unhandled_finalized: Default::default(),
        local_config: Config { deploy_delay },
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
    let ttl = TimeDiff::from(Duration::from_millis(100));
    let block_time1 = Timestamp::from(80);
    let block_time2 = Timestamp::from(120);
    let block_time3 = Timestamp::from(220);

    let no_deploys = HashSet::new();
    let mut proposer = create_test_proposer(0.into());
    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let deploy2 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let deploy3 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let deploy4 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );

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
    proposer.add_deploy_or_transfer(block_time2, *deploy1.id(), deploy1.deploy_type().unwrap());
    proposer.add_deploy_or_transfer(block_time2, *deploy2.id(), deploy2.deploy_type().unwrap());

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
    assert!(deploys.contains(&deploy1.id()));
    assert!(deploys.contains(&deploy2.id()));

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
    proposer.add_deploy_or_transfer(block_time2, *deploy3.id(), deploy3.deploy_type().unwrap());
    proposer.add_deploy_or_transfer(block_time2, *deploy4.id(), deploy4.deploy_type().unwrap());

    let block =
        proposer.propose_proto_block(DeployConfig::default(), block_time2, no_deploys, true);
    let deploys = block.deploys();

    // since block 1 is now finalized, neither deploy1 nor deploy2 should be among the returned
    assert_eq!(deploys.len(), 2);
    assert!(deploys.contains(&deploy3.id()));
    assert!(deploys.contains(&deploy4.id()));
}

#[test]
fn should_successfully_prune() {
    let expired_time = Timestamp::from(201);
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(Duration::from_millis(100));

    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let deploy2 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let deploy3 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let deploy4 = generate_deploy(
        &mut rng,
        creation_time + Duration::from_secs(20).into(),
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let mut proposer = create_test_proposer(0.into());

    // pending
    proposer.add_deploy_or_transfer(creation_time, *deploy1.id(), deploy1.deploy_type().unwrap());
    proposer.add_deploy_or_transfer(creation_time, *deploy2.id(), deploy2.deploy_type().unwrap());
    proposer.add_deploy_or_transfer(creation_time, *deploy3.id(), deploy3.deploy_type().unwrap());
    proposer.add_deploy_or_transfer(creation_time, *deploy4.id(), deploy4.deploy_type().unwrap());

    // pending => finalized
    proposer.finalized_deploys(vec![*deploy1.id()]);

    assert_eq!(proposer.sets.pending.len(), 3);
    assert!(proposer.sets.finalized_deploys.contains_key(deploy1.id()));

    // test for retained values
    let pruned = proposer.prune(test_time);
    assert_eq!(pruned, 0);

    assert_eq!(proposer.sets.pending.len(), 3);
    assert_eq!(proposer.sets.finalized_deploys.len(), 1);
    assert!(proposer.sets.finalized_deploys.contains_key(&deploy1.id()));

    // now move the clock to make some things expire
    let pruned = proposer.prune(expired_time);
    assert_eq!(pruned, 3);

    assert_eq!(proposer.sets.pending.len(), 1); // deploy4 is still valid
    assert_eq!(proposer.sets.finalized_deploys.len(), 0);
}

#[test]
fn should_keep_track_of_unhandled_deploys() {
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(Duration::from_millis(100));

    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let deploy2 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let mut proposer = create_test_proposer(0.into());

    // We do NOT add deploy2...
    proposer.add_deploy_or_transfer(creation_time, *deploy1.id(), deploy1.deploy_type().unwrap());
    // But we DO mark it as finalized, by it's hash
    proposer.finalized_deploys(vec![*deploy1.id(), *deploy2.id()]);

    assert!(
        proposer.contains_finalized(deploy1.id()),
        "should contain deploy1"
    );
    assert!(
        proposer.contains_finalized(deploy2.id()),
        "deploy2's hash should be considered seen"
    );
    assert!(
        !proposer.sets.finalized_deploys.contains_key(deploy2.id()),
        "should not yet contain deploy2"
    );
    assert!(
        proposer.contains_finalized(deploy2.id()),
        "should recognize deploy2 as finalized"
    );

    assert!(
        deploy2
            .header()
            .is_valid(&proposer.deploy_config, test_time),
        "deploy2 should be valid"
    );

    // Now we add Deploy2
    proposer.add_deploy_or_transfer(creation_time, *deploy2.id(), deploy2.deploy_type().unwrap());
    assert!(
        proposer.sets.finalized_deploys.contains_key(deploy2.id()),
        "deploy2 should now be in finalized_deploys"
    );
    assert!(
        !proposer.unhandled_finalized.contains(deploy2.id()),
        "deploy2 should not be in unhandled_finalized"
    );
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
        deploy_count: 30,
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
        deploy_count: 30,
        max_deploy_count: 20,
        proposed_count: 40,
        remaining_pending_count: 20,
        ..Default::default()
    });
}

#[test]
fn should_respect_limits_for_gas_cost() {
    test_proposer_with(TestArgs {
        transfer_count: 15,
        max_transfer_count: 20,
        deploy_count: 30,
        max_deploy_count: 20,
        payment_amount: default_gas_payment(),
        block_gas_limit: 10,
        proposed_count: 25,
        remaining_pending_count: 20,
        ..Default::default()
    });
}

#[test]
fn should_respect_block_gas_limit_for_deploys() {
    test_proposer_with(TestArgs {
        deploy_count: 15,
        payment_amount: default_gas_payment(),
        block_gas_limit: 5,
        max_deploy_count: 15,
        proposed_count: 5,
        remaining_pending_count: 10,
        ..Default::default()
    });
}

#[test]
fn should_propose_deploy_if_block_size_limit_met() {
    test_proposer_with(TestArgs {
        transfer_count: 1,
        deploy_count: 1,
        payment_amount: default_gas_payment(),
        block_gas_limit: 10,
        max_transfer_count: 2,
        max_deploy_count: 2,
        proposed_count: 2,
        remaining_pending_count: 0,
        max_block_size: Some(2 * DEPLOY_APPROX_MIN_SIZE),
    });
}

#[test]
fn should_not_propose_deploy_if_block_size_limit_within_threshold() {
    test_proposer_with(TestArgs {
        transfer_count: 2,
        deploy_count: 2,
        payment_amount: default_gas_payment(),
        block_gas_limit: 10,
        max_transfer_count: 3,
        max_deploy_count: 3,
        proposed_count: 4,
        remaining_pending_count: 0,
        max_block_size: Some(2 * DEPLOY_APPROX_MIN_SIZE),
    });
}

#[test]
fn should_not_propose_deploy_if_block_size_limit_passed() {
    test_proposer_with(TestArgs {
        deploy_count: 3,
        transfer_count: 2, // transfers should -not- count towards the block size limit
        payment_amount: default_gas_payment(),
        block_gas_limit: 100,
        max_transfer_count: 5,
        max_deploy_count: 5,
        proposed_count: 4,
        remaining_pending_count: 1,
        max_block_size: Some(2 * DEPLOY_APPROX_MIN_SIZE),
    });
}

#[test]
fn should_allow_transfers_to_exceed_block_size_limit() {
    test_proposer_with(TestArgs {
        deploy_count: 3,
        transfer_count: 60,
        payment_amount: default_gas_payment(),
        block_gas_limit: 100,
        max_transfer_count: 40,
        max_deploy_count: 5,
        proposed_count: 42,
        remaining_pending_count: 21,
        max_block_size: Some(2 * DEPLOY_APPROX_MIN_SIZE),
    });
}

#[derive(Default)]
struct TestArgs {
    /// Number of deploys to create.
    deploy_count: u32,
    /// Max deploys to propose.
    max_deploy_count: u32,
    /// Number of transfer deploys to create.
    transfer_count: u32,
    /// Number of transfer deploys to create.
    max_transfer_count: u32,
    /// Payment amount for transfers.
    payment_amount: Gas,
    /// Max gas cost for block.
    block_gas_limit: u64,
    /// Post-finalization of proposed block, how many transfers and deploys remain.
    remaining_pending_count: usize,
    /// Block deploy count proposed.
    proposed_count: usize,
    /// Block size limit in bytes.
    max_block_size: Option<usize>,
}

/// Test the block_proposer by generating deploys and transfers with variable limits, asserting
/// on internal counts post-finalization.
fn test_proposer_with(
    TestArgs {
        deploy_count,
        max_deploy_count,
        transfer_count,
        max_transfer_count,
        payment_amount,
        block_gas_limit,
        remaining_pending_count,
        proposed_count,
        max_block_size,
    }: TestArgs,
) -> BlockProposerReady {
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(Duration::from_millis(100));
    let past_deploys = HashSet::new();

    let mut rng = crate::new_rng();
    let mut proposer = create_test_proposer(0.into());
    let mut config = proposer.deploy_config;
    // defaults are 10, 1000 respectively
    config.block_max_deploy_count = max_deploy_count;
    config.block_max_transfer_count = max_transfer_count;
    config.block_gas_limit = block_gas_limit;
    if let Some(max_block_size) = max_block_size {
        config.max_block_size = max_block_size as u32;
    }

    for _ in 0..deploy_count {
        let deploy = generate_deploy(
            &mut rng,
            creation_time,
            ttl,
            vec![],
            payment_amount,
            DEFAULT_TEST_GAS_PRICE,
        );
        println!("generated deploy with size {}", deploy.serialized_length());
        proposer.add_deploy_or_transfer(creation_time, *deploy.id(), deploy.deploy_type().unwrap());
    }
    for _ in 0..transfer_count {
        let transfer = generate_transfer(&mut rng, creation_time, ttl, vec![], payment_amount);
        proposer.add_deploy_or_transfer(
            creation_time,
            *transfer.id(),
            transfer.deploy_type().unwrap(),
        );
    }

    let block = proposer.propose_proto_block(config, test_time, past_deploys, true);
    let all_deploys = BlockLike::deploys(&block);
    proposer.finalized_deploys(all_deploys.iter().map(|hash| **hash));
    println!("proposed deploys {}", block.wasm_deploys().len());
    println!("proposed transfers {}", block.transfers().len());
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
    let ttl = TimeDiff::from(Duration::from_millis(100));
    let block_time = Timestamp::from(120);

    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    // let deploy2 depend on deploy1
    let deploy2 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![*deploy1.id()],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );

    let no_deploys = HashSet::new();
    let mut proposer = create_test_proposer(0.into());

    // add deploy2
    proposer.add_deploy_or_transfer(creation_time, *deploy2.id(), deploy2.deploy_type().unwrap());

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
    proposer.add_deploy_or_transfer(creation_time, *deploy1.id(), deploy1.deploy_type().unwrap());

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
    assert!(deploys.contains(deploy1.id()));

    // the deploy will be included in block 1
    proposer.finalized_deploys(deploys.iter().copied());

    let block = proposer.propose_proto_block(DeployConfig::default(), block_time, no_deploys, true);
    // `blocks` contains a block that contains deploy1 now, so we should get deploy2
    let deploys2 = block.wasm_deploys();
    assert_eq!(deploys2.len(), 1);
    assert!(deploys2.contains(deploy2.id()));
}

#[test]
fn should_respect_deploy_delay() {
    let mut rng = crate::new_rng();
    let creation_time = Timestamp::from(0);
    let ttl = TimeDiff::from(10000);
    let no_deploys = HashSet::new();
    let deploy_config = DeployConfig::default();
    let deploy = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        vec![],
        default_gas_payment(),
        DEFAULT_TEST_GAS_PRICE,
    );
    let mut proposer = create_test_proposer(10.into()); // Deploy delay: 10 milliseconds

    // Add the deploy at time 100. So at 109 it cannot be proposed yet, but at time 110 it can.
    proposer.add_deploy_or_transfer(100.into(), *deploy.id(), deploy.deploy_type().unwrap());
    let block = proposer.propose_proto_block(deploy_config, 109.into(), no_deploys.clone(), true);
    assert!(block.wasm_deploys().is_empty());
    let block = proposer.propose_proto_block(deploy_config, 110.into(), no_deploys, true);
    assert_eq!(&vec![*deploy.id()], block.wasm_deploys());
}
