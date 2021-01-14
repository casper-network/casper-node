use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    runtime_args,
    standard_payment::ARG_AMOUNT,
    RuntimeArgs, SecretKey,
};

use crate::{
    crypto::AsymmetricKeyExt,
    testing::TestRng,
    types::{BlockLike, Deploy, DeployHash, TimeDiff},
};

use super::*;

fn default_gas_payment() -> Gas {
    Gas::from(1u32)
}

// gas_price used for test deploys
const TEST_GAS_PRICE: u64 = 1;

fn generate_transfer(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
    gas_price: u64,
) -> Deploy {
    generate_transfer_with_deps(rng, timestamp, ttl, vec![], gas_price)
}

fn generate_transfer_with_deps(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
    dependencies: Vec<DeployHash>,
    gas_price: u64,
) -> Deploy {
    let secret_key = SecretKey::random(rng);
    let chain_name = "chain".to_string();

    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: Default::default(),
    };

    let session = ExecutableDeployItem::Transfer { args: Bytes::new() };

    Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
        rng,
    )
}

fn generate_deploy(
    rng: &mut TestRng,
    timestamp: Timestamp,
    ttl: TimeDiff,
    payment_amount: Gas,
    gas_price: u64,
) -> Deploy {
    generate_deploy_with_deps(rng, timestamp, ttl, vec![], payment_amount, gas_price)
}

fn generate_deploy_with_deps(
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
    }
    .to_bytes()
    .expect("should serialize");
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: args.into(),
    };
    let session = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: Bytes::new(),
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
        rng,
    )
}

fn create_test_proposer(wasmless_transfer_cost: u64) -> BlockProposerReady {
    BlockProposerReady {
        sets: Default::default(),
        deploy_config: Default::default(),
        wasmless_transfer_cost,
        state_key: b"block-proposer-test".to_vec(),
        request_queue: Default::default(),
        unhandled_finalized: Default::default(),
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
    let mut proposer = create_test_proposer(0);
    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        default_gas_payment(),
        TEST_GAS_PRICE,
    );
    let deploy2 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        default_gas_payment(),
        TEST_GAS_PRICE,
    );
    let deploy3 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        default_gas_payment(),
        TEST_GAS_PRICE,
    );
    let deploy4 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        default_gas_payment(),
        TEST_GAS_PRICE,
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

    assert_eq!(deploys.len(), 2, "should get 2 deploys proposed");
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
fn should_not_add_wasm_deploy_with_zero_gas_price() {
    let creation_time = Timestamp::from(100);
    let ttl = TimeDiff::from(Duration::from_millis(100));

    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(&mut rng, creation_time, ttl, default_gas_payment(), 0);
    let mut proposer = create_test_proposer(0);
    // pending
    proposer.add_deploy_or_transfer(creation_time, *deploy1.id(), deploy1.deploy_type().unwrap());
    assert_eq!(proposer.sets.pending.len(), 0, "there should be 0 pending deploys - the one added has a gas price (willingness to pay) of 0");
}

#[test]
fn should_add_transfers() {
    let creation_time = Timestamp::from(100);
    let ttl = TimeDiff::from(Duration::from_millis(100));

    let mut rng = crate::new_rng();
    let mut proposer = create_test_proposer(1);
    let deploy1 = generate_transfer(
        &mut rng,
        creation_time,
        ttl,
        proposer.wasmless_transfer_cost,
    );

    assert_eq!(
        deploy1.header().gas_price(),
        proposer.wasmless_transfer_cost,
        "transfer's gas cost should be the same as specified in the proposer"
    );
    // pending
    proposer.add_deploy_or_transfer(creation_time, *deploy1.id(), deploy1.deploy_type().unwrap());
    assert_eq!(
        proposer.sets.pending.len(),
        1,
        "there should be 1 pending deploys"
    );
}

#[test]
fn should_successfully_prune() {
    let expired_time = Timestamp::from(201);
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(Duration::from_millis(100));

    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(&mut rng, creation_time, ttl, default_gas_payment(), 1);
    let deploy2 = generate_deploy(&mut rng, creation_time, ttl, default_gas_payment(), 1);
    let deploy3 = generate_deploy(&mut rng, creation_time, ttl, default_gas_payment(), 1);
    let deploy4 = generate_deploy(
        &mut rng,
        creation_time + Duration::from_secs(20).into(),
        ttl,
        default_gas_payment(),
        TEST_GAS_PRICE,
    );
    let mut proposer = create_test_proposer(0);

    // pending
    proposer.add_deploy_or_transfer(creation_time, *deploy1.id(), deploy1.deploy_type().unwrap());
    proposer.add_deploy_or_transfer(creation_time, *deploy2.id(), deploy2.deploy_type().unwrap());
    proposer.add_deploy_or_transfer(creation_time, *deploy3.id(), deploy3.deploy_type().unwrap());
    proposer.add_deploy_or_transfer(creation_time, *deploy4.id(), deploy4.deploy_type().unwrap());
    assert_eq!(
        proposer.sets.pending.len(),
        4,
        "there should be 4 pending deploys"
    );

    // pending => finalized
    proposer.finalized_deploys(vec![*deploy1.id()]);

    assert_eq!(
        proposer.sets.pending.len(),
        3,
        "there should be 3 pending deploys"
    );
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
        default_gas_payment(),
        TEST_GAS_PRICE,
    );
    let deploy2 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        default_gas_payment(),
        TEST_GAS_PRICE,
    );
    let mut proposer = create_test_proposer(0);

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

    let past_deploys = HashSet::new();
    assert!(
        proposer.is_deploy_valid(
            deploy2.header(),
            test_time,
            &proposer.deploy_config,
            &past_deploys
        ),
        "deploy2 should -not- be valid"
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
        expected: TestValues {
            proposed_count: 20,
            pending_count: 10,
        },
        ..Default::default()
    });
}

#[test]
fn should_respect_limits_for_wasm_deploys() {
    test_proposer_with(TestArgs {
        deploy_count: 30,
        max_deploy_count: 20,
        expected: TestValues {
            proposed_count: 20,
            pending_count: 10,
        },
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
        expected: TestValues {
            proposed_count: 40,
            pending_count: 20,
        },
        ..Default::default()
    });
}

#[test]
fn should_prioritize_deploys_based_on_gas_price_and_payment_amount() {
    let mut proposer = create_test_proposer(5);
    let mut config = proposer.deploy_config;
    config.block_max_deploy_count = 30;
    config.block_max_transfer_count = 50;
    config.block_gas_limit = 1000;

    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(Duration::from_millis(100));
    let mut rng = crate::new_rng();

    let gen_deploy = |r: &mut TestRng, gas_price, payment| {
        generate_deploy(r, creation_time, ttl, payment, gas_price)
    };
    let gen_transfer =
        |r: &mut TestRng, gas_price| generate_transfer(r, creation_time, ttl, gas_price);

    let r = &mut rng;
    let mut transfers = Vec::new();
    for _ in 0..40 {
        transfers.push(gen_transfer(r, 1));
    }

    // 5 more deploys are sent in with a higher gas price
    for _ in 0..5 {
        transfers.push(gen_transfer(r, 10));
    }

    let mut deploys = Vec::new();
    for _ in 0..20 {
        deploys.push(gen_deploy(r, 1, Gas::from(50u32)))
    }

    for _ in 0..5 {
        deploys.push(gen_deploy(r, 1, Gas::from(100u32)));
    }

    let block = propose_deploys(
        &mut proposer,
        config,
        deploys,
        transfers,
        creation_time,
        test_time,
    );

    // Our (sorted by gas_price, then payment_amount) list looks like this:
    //  (
    //    [ 5  transfers with gas_price=10, payment_amount=5   ]
    //    [ 5  deploys   with gas_price=1,  payment_amount=100 ]
    //    [ 20 deploys   with gas_price=1,  payment_amount=50  ]
    //    [ 40 transfers with gas_price=1,  payment_amount=5   ]
    //  )

    // 5 transfers chose higher gas_price, and are therefore picked first, and finally 5 more are
    // chosen after wasm deploys.
    assert_eq!(
        block.transfers().len(),
        5 + 5,
        "should have seen N transfers, proposed: {:#?}",
        block
    );

    assert_eq!(
        block.wasm_deploys().len(),
        5 + 9,
        "should have seen N wasm_deploys, proposed {:#?}",
        block
    );

    assert_eq!(
        proposer.sets.pending.len(),
        46,
        "should have seen N pending deploys"
    );

    let all_deploys = BlockLike::deploys(&block)
        .iter()
        .map(|dh| **dh)
        .collect::<Vec<_>>();
    proposer.finalized_deploys(all_deploys);

    let next_block = proposer.propose_proto_block(config, creation_time, HashSet::new(), true);
    assert_eq!(
        next_block.wasm_deploys().len(),
        11,
        "should have N wasm deploys",
    );
    assert_eq!(next_block.transfers().len(), 35, "should have N transfers");
    let all_deploys = BlockLike::deploys(&next_block)
        .iter()
        .map(|dh| **dh)
        .collect::<Vec<_>>();
    proposer.finalized_deploys(all_deploys);

    assert_eq!(
        proposer.sets.pending.len(),
        0,
        "N deploys should remain pending {:#?}",
        proposer.sets.pending
    );
}

#[test]
fn should_respect_limits_for_gas_cost() {
    test_proposer_with(TestArgs {
        transfer_count: 35,
        max_transfer_count: 20,
        deploy_count: 30,
        max_deploy_count: 20,
        block_gas_limit: 25,
        expected: TestValues {
            proposed_count: 25,
            pending_count: 40,
        },
        ..Default::default()
    });
}

#[test]
fn should_respect_block_gas_limit_for_transfers() {
    test_proposer_with(TestArgs {
        wasmless_transfer_cost: 5,
        transfer_count: 15,
        block_gas_limit: 20,
        max_transfer_count: 15,
        expected: TestValues {
            proposed_count: 4,
            pending_count: 11,
        },
        ..Default::default()
    });
}

#[test]
fn should_respect_block_gas_limit_for_deploys() {
    test_proposer_with(TestArgs {
        deploy_count: 15,
        block_gas_limit: 5,
        max_deploy_count: 15,
        expected: TestValues {
            proposed_count: 5,
            pending_count: 10,
        },
        ..Default::default()
    });
}

#[test]
fn should_propose_deploy_if_block_size_limit_met() {
    test_proposer_with(TestArgs {
        transfer_count: 1,
        deploy_count: 1,
        expected: TestValues {
            proposed_count: 2,
            pending_count: 0,
        },
        max_block_size: Some(2 * DEPLOY_APPROX_MIN_SIZE),
        ..Default::default()
    });
}

#[test]
fn should_not_propose_deploy_if_block_size_limit_within_threshold() {
    test_proposer_with(TestArgs {
        transfer_count: 2,
        deploy_count: 2,
        max_block_size: Some(2 * DEPLOY_APPROX_MIN_SIZE),
        expected: TestValues {
            proposed_count: 2,
            pending_count: 2,
        },
        ..Default::default()
    });
}

#[test]
fn should_not_propose_deploy_if_block_size_limit_passed() {
    test_proposer_with(TestArgs {
        deploy_count: 0,
        transfer_count: 1,
        block_gas_limit: 10,
        max_transfer_count: 5,
        max_block_size: Some(100usize),
        expected: TestValues {
            proposed_count: 0,
            pending_count: 1,
        },
        ..Default::default()
    });
}

struct TestArgs {
    /// Wasm-less transfer cost.
    wasmless_transfer_cost: u64,
    /// gas_price used for deploys.
    gas_price: u64,
    /// Number of deploys to create.
    deploy_count: u32,
    /// Deploy payment_amount.
    deploy_payment_amount: Gas,
    /// Max deploys to propose.
    max_deploy_count: u32,
    /// Number of transfer deploys to create.
    transfer_count: u32,
    /// Number of transfer deploys to create.
    max_transfer_count: u32,
    /// Max gas cost for block.
    block_gas_limit: u64,
    /// Block size limit in bytes.
    max_block_size: Option<usize>,
    /// Expected values for a given test
    expected: TestValues,
}

impl Default for TestArgs {
    fn default() -> Self {
        TestArgs {
            transfer_count: 0,
            deploy_count: 0,
            wasmless_transfer_cost: 1,
            gas_price: 1u64,
            deploy_payment_amount: Gas::from(1u32),
            max_deploy_count: 1000,
            max_transfer_count: 1000,
            block_gas_limit: 1000,
            max_block_size: None,
            expected: Default::default(),
        }
    }
}

#[derive(Default, Debug, PartialEq)]
struct TestValues {
    /// Post-finalization of proposed block, how many transfers and deploys remain.
    pending_count: usize,
    /// Block deploy count proposed.
    proposed_count: usize,
}

fn propose_deploys(
    proposer: &mut BlockProposerReady,
    config: DeployConfig,
    deploys: Vec<Deploy>,
    transfers: Vec<Deploy>,
    creation_time: Timestamp,
    test_time: Timestamp,
) -> ProtoBlock {
    let past_deploys = HashSet::new();

    for deploy in deploys {
        proposer.add_deploy_or_transfer(creation_time, *deploy.id(), deploy.deploy_type().unwrap());
    }
    for transfer in transfers {
        proposer.add_deploy_or_transfer(
            creation_time,
            *transfer.id(),
            transfer.deploy_type().unwrap(),
        );
    }
    let block = proposer.propose_proto_block(config, test_time, past_deploys, true);
    let all_deploys = BlockLike::deploys(&block);
    proposer.finalized_deploys(all_deploys.iter().map(|hash| **hash));
    block
}

/// Test the block_proposer by generating deploys and transfers with variable limits, asserting
/// on internal counts post-finalization.
fn test_proposer_with(
    TestArgs {
        wasmless_transfer_cost,
        deploy_count,
        max_deploy_count,
        transfer_count,
        max_transfer_count,
        gas_price,
        deploy_payment_amount,
        block_gas_limit,
        expected,
        max_block_size,
    }: TestArgs,
) -> BlockProposerReady {
    let creation_time = Timestamp::from(100);
    let test_time = Timestamp::from(120);
    let ttl = TimeDiff::from(Duration::from_millis(100));
    let past_deploys = HashSet::new();

    let mut rng = crate::new_rng();
    let mut proposer = create_test_proposer(wasmless_transfer_cost);
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
            deploy_payment_amount,
            gas_price,
        );
        // println!("generated deploy with size {}", deploy.serialized_length());
        proposer.add_deploy_or_transfer(creation_time, *deploy.id(), deploy.deploy_type().unwrap());
    }
    assert_eq!(
        proposer.sets.pending.len(),
        deploy_count as usize,
        "should have {} pending deploys",
        deploy_count
    );

    for _ in 0..transfer_count {
        let transfer = generate_transfer(&mut rng, creation_time, ttl, gas_price);
        // println!( "generated transfer with size {}", transfer.serialized_length() );
        proposer.add_deploy_or_transfer(
            creation_time,
            *transfer.id(),
            transfer.deploy_type().unwrap(),
        );
    }
    assert_eq!(
        proposer.sets.pending.len(),
        (deploy_count + transfer_count) as usize,
        "should have {} pending deploys",
        deploy_count + transfer_count
    );

    let block = proposer.propose_proto_block(config, test_time, past_deploys, true);
    let all_deploys = BlockLike::deploys(&block);
    proposer.finalized_deploys(all_deploys.iter().map(|hash| **hash));

    let results = TestValues {
        pending_count: proposer.sets.pending.len(),
        proposed_count: all_deploys.len(),
    };
    assert_eq!(
        results, expected,
        "expected {:?}, but got {:?}",
        expected, results
    );

    assert!(
        block.wasm_deploys().len() <= max_deploy_count as usize,
        "proposed too many wasm deploys ({})",
        block.wasm_deploys().len()
    );
    assert!(
        block.transfers().len() <= max_transfer_count as usize,
        "proposed too many transfer deploys ({})",
        block.transfers().len()
    );
    proposer
}

#[test]
fn should_not_propose_deploy_if_missing_deps() {
    let creation_time = Timestamp::from(100);
    let ttl = TimeDiff::from(Duration::from_millis(100));
    let block_time = Timestamp::from(120);

    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        default_gas_payment(),
        TEST_GAS_PRICE,
    );
    // let deploy2 depend on deploy1
    let deploy2 = generate_deploy_with_deps(
        &mut rng,
        creation_time,
        ttl,
        vec![*deploy1.id()],
        default_gas_payment(),
        0,
    );

    let no_deploys = HashSet::new();
    let mut proposer = create_test_proposer(0);

    // add deploy2
    proposer.add_deploy_or_transfer(creation_time, *deploy2.id(), deploy2.deploy_type().unwrap());

    // deploy2 has an unsatisfied dependency
    assert!(proposer
        .propose_proto_block(
            DeployConfig::default(),
            block_time,
            no_deploys,
            true
        )
        .deploys()
        .is_empty());
}

#[test]
fn should_retain_deploys_with_unmet_dependencies_for_later_proposal() {
    let creation_time = Timestamp::from(100);
    let ttl = TimeDiff::from(Duration::from_millis(100));
    let block_time = Timestamp::from(120);

    let mut rng = crate::new_rng();
    let deploy1 = generate_deploy(
        &mut rng,
        creation_time,
        ttl,
        default_gas_payment(),
        TEST_GAS_PRICE,
    );
    let deploy2 = generate_deploy_with_deps(
        &mut rng,
        creation_time,
        ttl,
        vec![*deploy1.id()],
        default_gas_payment(),
        TEST_GAS_PRICE,
    );

    let no_deploys = HashSet::new();
    let mut proposer = create_test_proposer(0);

    proposer.add_deploy_or_transfer(creation_time, *deploy2.id(), deploy2.deploy_type().unwrap());
    assert_eq!(
        proposer.sets.pending.len(),
        1,
        "deploy2 should be in pending"
    );

    // 1. deploy2 has an unsatisfied dependency so no deploys are proposed.
    assert!(proposer
        .propose_proto_block(
            DeployConfig::default(),
            block_time,
            no_deploys.clone(),
            true
        )
        .wasm_deploys()
        .is_empty());
    assert_eq!(
        proposer.sets.pending.len(),
        1,
        "deploy with unmet deps should remain in pending"
    );

    proposer.add_deploy_or_transfer(creation_time, *deploy1.id(), deploy1.deploy_type().unwrap());

    // 2. deploy1 has no unsatisfied dependencies and will be proposed.
    let block = proposer.propose_proto_block(
        DeployConfig::default(),
        block_time,
        no_deploys.clone(),
        true,
    );
    // 3. only deploy1 should be proposed, as it has no dependencies
    assert_eq!(block.wasm_deploys().len(), 1);
    assert!(
        block.wasm_deploys().contains(deploy1.id()),
        "block should contain deploy"
    );

    // 4. deploy1 will be finalized.
    proposer.finalized_deploys(vec![*deploy1.id()]);
    assert!(
        proposer.sets.finalized_deploys.contains_key(deploy1.id()),
        "proposer should have tracked finalized deploy1 now"
    );

    // 5. pending should now only contain deploy2
    assert_eq!(proposer.sets.pending.len(), 1);

    // 6. finally, propose a block, and deploy2 should be included from pending, because it's
    // dependency, deploy1, was included in a past block.
    let block = proposer.propose_proto_block(DeployConfig::default(), block_time, no_deploys, true);
    assert_eq!(
        block.wasm_deploys().len(),
        1,
        "should have 1 deploy, (deploy2)"
    );
    assert!(
        block.wasm_deploys().contains(deploy2.id()),
        "should have 1 deploy, (deploy2)"
    );
}
