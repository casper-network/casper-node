use std::{collections::HashMap, sync::Arc, time::Duration};

use casper_binary_port::{
    BinaryMessage, BinaryMessageCodec, BinaryResponseAndRequest, Command, CommandHeader,
    InformationRequest, Uptime,
};
use either::Either;
use futures::{SinkExt, StreamExt};
use num_rational::Ratio;
use tokio::{
    net::TcpStream,
    time::{self, timeout},
};
use tokio_util::codec::Framed;
use tracing::info;

use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    execution::TransformKindV2,
    system::{auction::BidAddr, AUCTION},
    testing::TestRng,
    AvailableBlockRange, Deploy, Key, Peers, PublicKey, SecretKey, StoredValue, TimeDiff,
    Timestamp, Transaction,
};

use crate::{
    effect::{requests::ContractRuntimeRequest, EffectExt},
    reactor::{
        main_reactor::{
            tests::{
                configs_override::{ConfigsOverride, NodeConfigOverride},
                fixture::TestFixture,
                initial_stakes::InitialStakes,
                node_has_lowest_available_block_at_or_below_height, Nodes, ERA_ONE, ERA_THREE,
                ERA_TWO, ERA_ZERO, ONE_MIN, TEN_SECS, THIRTY_SECS, TWO_MIN,
            },
            MainEvent, MainReactor, ReactorState,
        },
        Runner,
    },
    testing::{filter_reactor::FilterReactor, network::TestingNetwork, ConditionCheckReactor},
    types::{ExitCode, NodeId, SyncHandling},
    utils::Source,
};

#[tokio::test]
async fn run_network() {
    // Set up a network with five nodes and run until in era 2.
    let initial_stakes = InitialStakes::Random { count: 5 };
    let mut fixture = TestFixture::new(initial_stakes, None).await;
    fixture.run_until_consensus_in_era(ERA_TWO, ONE_MIN).await;
}

#[tokio::test]
async fn historical_sync_with_era_height_1() {
    let initial_stakes = InitialStakes::Random { count: 5 };
    let spec_override = ConfigsOverride {
        minimum_block_time: "4seconds".parse().unwrap(),
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    // Wait for all nodes to reach era 3.
    fixture.run_until_consensus_in_era(ERA_THREE, TWO_MIN).await;

    // Create a joiner node.
    let secret_key = SecretKey::random(&mut fixture.rng);
    let trusted_hash = *fixture.highest_complete_block().hash();
    let (mut config, storage_dir) = fixture.create_node_config(
        &secret_key,
        Some(trusted_hash),
        1,
        NodeConfigOverride::default(),
    );
    config.node.sync_handling = SyncHandling::Genesis;
    let joiner_id = fixture
        .add_node(Arc::new(secret_key), config, storage_dir)
        .await;

    // Wait for joiner node to sync back to the block from era 1
    fixture
        .run_until(
            node_has_lowest_available_block_at_or_below_height(1, joiner_id),
            ONE_MIN,
        )
        .await;

    // Remove the weights for era 0 and era 1 from the validator matrix
    let runner = fixture
        .network
        .nodes_mut()
        .get_mut(&joiner_id)
        .expect("Could not find runner for node {joiner_id}");
    let reactor = runner.reactor_mut().inner_mut().inner_mut();
    reactor.validator_matrix.purge_era_validators(&ERA_ZERO);
    reactor.validator_matrix.purge_era_validators(&ERA_ONE);

    // Continue syncing and check if the joiner node reaches era 0
    fixture
        .run_until(
            node_has_lowest_available_block_at_or_below_height(0, joiner_id),
            ONE_MIN,
        )
        .await;
}

#[tokio::test]
async fn should_not_historical_sync_no_sync_node() {
    let initial_stakes = InitialStakes::Random { count: 5 };
    let spec_override = ConfigsOverride {
        minimum_block_time: "4seconds".parse().unwrap(),
        minimum_era_height: 2,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    // Wait for all nodes to complete block 1.
    fixture.run_until_block_height(1, ONE_MIN).await;

    // Create a joiner node.
    let highest_block = fixture.highest_complete_block();
    let trusted_hash = *highest_block.hash();
    let trusted_height = highest_block.height();
    assert!(
        trusted_height > 0,
        "trusted height must be non-zero to allow for checking that the joiner doesn't do \
        historical syncing"
    );
    info!("joining node using block {trusted_height} {trusted_hash}");
    let secret_key = SecretKey::random(&mut fixture.rng);
    let (mut config, storage_dir) = fixture.create_node_config(
        &secret_key,
        Some(trusted_hash),
        1,
        NodeConfigOverride::default(),
    );
    config.node.sync_handling = SyncHandling::NoSync;
    let joiner_id = fixture
        .add_node(Arc::new(secret_key), config, storage_dir)
        .await;

    let joiner_avail_range = |nodes: &Nodes| {
        nodes
            .get(&joiner_id)
            .expect("should have joiner")
            .main_reactor()
            .storage()
            .get_available_block_range()
    };

    // Run until the joiner doesn't have the default available block range, i.e. it has completed
    // syncing the initial block.
    fixture
        .try_run_until(
            |nodes: &Nodes| joiner_avail_range(nodes) != AvailableBlockRange::RANGE_0_0,
            ONE_MIN,
        )
        .await
        .expect("timed out waiting for joiner to sync first block");

    let available_block_range_pre = joiner_avail_range(fixture.network.nodes());

    let pre = available_block_range_pre.low();
    assert!(
        pre >= trusted_height,
        "should not have acquired a block earlier than trusted hash block {} {}",
        pre,
        trusted_height
    );

    // Ensure the joiner's chain is advancing.
    fixture
        .try_run_until(
            |nodes: &Nodes| joiner_avail_range(nodes).high() > available_block_range_pre.high(),
            ONE_MIN,
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "timed out waiting for joiner's highest complete block to exceed {}",
                available_block_range_pre.high()
            )
        });

    // Ensure the joiner is not doing historical sync.
    fixture
        .try_run_until(
            |nodes: &Nodes| joiner_avail_range(nodes).low() < available_block_range_pre.low(),
            TEN_SECS,
        )
        .await
        .unwrap_err();
}

#[tokio::test]
async fn should_catch_up_and_shutdown() {
    let initial_stakes = InitialStakes::Random { count: 5 };
    let spec_override = ConfigsOverride {
        minimum_block_time: "4seconds".parse().unwrap(),
        minimum_era_height: 2,
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(spec_override)).await;

    // Wait for all nodes to complete block 1.
    fixture.run_until_block_height(1, ONE_MIN).await;

    // Create a joiner node.
    let highest_block = fixture.highest_complete_block();
    let trusted_hash = *highest_block.hash();
    let trusted_height = highest_block.height();
    assert!(
        trusted_height > 0,
        "trusted height must be non-zero to allow for checking that the joiner doesn't do \
        historical syncing"
    );

    info!("joining node using block {trusted_height} {trusted_hash}");
    let secret_key = SecretKey::random(&mut fixture.rng);
    let (mut config, storage_dir) = fixture.create_node_config(
        &secret_key,
        Some(trusted_hash),
        1,
        NodeConfigOverride::default(),
    );
    config.node.sync_handling = SyncHandling::CompleteBlock;
    let joiner_id = fixture
        .add_node(Arc::new(secret_key), config, storage_dir)
        .await;

    let joiner_avail_range = |nodes: &Nodes| {
        nodes
            .get(&joiner_id)
            .expect("should have joiner")
            .main_reactor()
            .storage()
            .get_available_block_range()
    };

    // Run until the joiner shuts down after catching up
    fixture
        .network
        .settle_on_node_exit(
            &mut fixture.rng,
            &joiner_id,
            ExitCode::CleanExitDontRestart,
            ONE_MIN,
        )
        .await;

    let available_block_range = joiner_avail_range(fixture.network.nodes());

    let low = available_block_range.low();
    assert!(
        low >= trusted_height,
        "should not have acquired a block earlier than trusted hash block {low} {trusted_hash}",
    );

    let highest_block_height = fixture.highest_complete_block().height();
    let high = available_block_range.high();
    assert!(
        low < high && high <= highest_block_height,
        "should have acquired more recent blocks before shutting down {low} {high} {highest_block_height}",
    );
}

fn network_is_in_keepup(
    nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<FilterReactor<MainReactor>>>>,
) -> bool {
    nodes
        .values()
        .all(|node| node.reactor().inner().inner().state == ReactorState::KeepUp)
}

const MESSAGE_SIZE: u32 = 1024 * 1024 * 10;

async fn setup_network_and_get_binary_port_handle(
    initial_stakes: InitialStakes,
    spec_override: ConfigsOverride,
) -> (
    Framed<TcpStream, BinaryMessageCodec>,
    impl futures::Future<Output = (TestingNetwork<FilterReactor<MainReactor>>, TestRng)>,
) {
    let mut fixture = timeout(
        Duration::from_secs(10),
        TestFixture::new(initial_stakes, Some(spec_override)),
    )
    .await
    .unwrap();
    let mut rng = fixture.rng_mut().create_child();
    let net = fixture.network_mut();
    net.settle_on(&mut rng, network_is_in_keepup, Duration::from_secs(59))
        .await;
    let (_, first_node) = net
        .nodes()
        .iter()
        .next()
        .expect("should have at least one node");
    let binary_port_addr = first_node
        .main_reactor()
        .binary_port
        .bind_address()
        .unwrap();
    let finish_cranking = fixture.run_until_stopped(rng.create_child());
    let address = format!("localhost:{}", binary_port_addr.port());
    let stream = TcpStream::connect(address.clone())
        .await
        .expect("should create stream");
    let client = Framed::new(stream, BinaryMessageCodec::new(MESSAGE_SIZE));
    (client, finish_cranking)
}

#[tokio::test]
async fn should_start_in_isolation() {
    let initial_stakes = InitialStakes::Random { count: 1 };
    let spec_override = ConfigsOverride {
        node_config_override: NodeConfigOverride {
            sync_handling_override: Some(SyncHandling::Isolated),
            idle_tolerance: None,
        },
        ..Default::default()
    };
    let (mut client, finish_cranking) =
        setup_network_and_get_binary_port_handle(initial_stakes, spec_override).await;

    let uptime_request_bytes = {
        let request = Command::Get(
            InformationRequest::Uptime
                .try_into()
                .expect("should convert"),
        );
        let header = CommandHeader::new(request.tag(), 1_u16);
        let header_bytes = ToBytes::to_bytes(&header).expect("should serialize");
        header_bytes
            .iter()
            .chain(
                ToBytes::to_bytes(&request)
                    .expect("should serialize")
                    .iter(),
            )
            .cloned()
            .collect::<Vec<_>>()
    };
    client
        .send(BinaryMessage::new(uptime_request_bytes))
        .await
        .expect("should send message");
    let response = timeout(Duration::from_secs(20), client.next())
        .await
        .unwrap_or_else(|err| panic!("should complete uptime request without timeout: {}", err))
        .unwrap_or_else(|| panic!("should have bytes"))
        .unwrap_or_else(|err| panic!("should have ok response: {}", err));
    let (binary_response_and_request, _): (BinaryResponseAndRequest, _) =
        FromBytes::from_bytes(response.payload()).expect("should deserialize response");
    let response = binary_response_and_request.response().payload();
    let (uptime, remainder): (Uptime, _) =
        FromBytes::from_bytes(response).expect("Peers should be deserializable");
    assert!(remainder.is_empty());
    assert!(uptime.into_inner() > 0);
    let (_net, _rng) = timeout(Duration::from_secs(20), finish_cranking)
        .await
        .unwrap_or_else(|_| panic!("should finish cranking without timeout"));
}

#[tokio::test]
async fn should_be_peerless_in_isolation() {
    let initial_stakes = InitialStakes::Random { count: 1 };
    let spec_override = ConfigsOverride {
        node_config_override: NodeConfigOverride {
            sync_handling_override: Some(SyncHandling::Isolated),
            idle_tolerance: None,
        },
        ..Default::default()
    };
    let (mut client, finish_cranking) =
        setup_network_and_get_binary_port_handle(initial_stakes, spec_override).await;

    let peers_request_bytes = {
        let request = Command::Get(
            InformationRequest::Peers
                .try_into()
                .expect("should convert"),
        );
        let header = CommandHeader::new(request.tag(), 1_u16);
        let header_bytes = ToBytes::to_bytes(&header).expect("should serialize");
        header_bytes
            .iter()
            .chain(
                ToBytes::to_bytes(&request)
                    .expect("should serialize")
                    .iter(),
            )
            .cloned()
            .collect::<Vec<_>>()
    };
    client
        .send(BinaryMessage::new(peers_request_bytes))
        .await
        .expect("should send message");
    let response = timeout(Duration::from_secs(20), client.next())
        .await
        .unwrap_or_else(|err| panic!("should complete peers request without timeout: {}", err))
        .unwrap_or_else(|| panic!("should have bytes"))
        .unwrap_or_else(|err| panic!("should have ok response: {}", err));
    let (binary_response_and_request, _): (BinaryResponseAndRequest, _) =
        FromBytes::from_bytes(response.payload()).expect("should deserialize response");
    let response = binary_response_and_request.response().payload();

    let (peers, remainder): (Peers, _) =
        FromBytes::from_bytes(response).expect("Peers should be deserializable");
    assert!(remainder.is_empty());
    assert!(
        peers.into_inner().is_empty(),
        "should not have peers in isolated mode"
    );

    let (_net, _rng) = timeout(Duration::from_secs(20), finish_cranking)
        .await
        .unwrap_or_else(|_| panic!("should finish cranking without timeout"));
}

#[tokio::test]
async fn network_should_recover_from_stall() {
    // Set up a network with three nodes.
    let initial_stakes = InitialStakes::AllEqual {
        count: 3,
        stake: 100,
    };
    let mut fixture = TestFixture::new(initial_stakes, None).await;

    // Let all nodes progress until block 2 is marked complete.
    fixture.run_until_block_height(2, ONE_MIN).await;

    // Kill all nodes except for node 0.
    let mut stopped_nodes = vec![];
    for _ in 1..fixture.node_contexts.len() {
        let node_context = fixture.remove_and_stop_node(1);
        stopped_nodes.push(node_context);
    }

    // Expect node 0 can't produce more blocks, i.e. the network has stalled.
    fixture
        .try_run_until_block_height(3, ONE_MIN)
        .await
        .expect_err("should time out");

    // Restart the stopped nodes.
    for node_context in stopped_nodes {
        fixture
            .add_node(
                node_context.secret_key,
                node_context.config,
                node_context.storage_dir,
            )
            .await;
    }

    // Ensure all nodes progress until block 3 is marked complete.
    fixture.run_until_block_height(3, TEN_SECS).await;
}

#[tokio::test]
async fn node_should_rejoin_after_ejection() {
    let initial_stakes = InitialStakes::AllEqual {
        count: 5,
        stake: 10_000_000_000,
    };
    let minimum_era_height = 4;
    let configs_override = ConfigsOverride {
        minimum_era_height,
        minimum_block_time: "4096 ms".parse().unwrap(),
        round_seigniorage_rate: Ratio::new(1, 1_000_000_000_000),
        ..Default::default()
    };
    let mut fixture = TestFixture::new(initial_stakes, Some(configs_override)).await;

    // Run through the first era.
    fixture
        .run_until_block_height(minimum_era_height, ONE_MIN)
        .await;

    let stopped_node = fixture.remove_and_stop_node(1);
    let stopped_secret_key = Arc::clone(&stopped_node.secret_key);
    let stopped_public_key = PublicKey::from(&*stopped_secret_key);

    // Wait until the stopped node is ejected and removed from the validators set.
    fixture
        .run_until_consensus_in_era(
            (fixture.chainspec.core_config.auction_delay + 3).into(),
            ONE_MIN,
        )
        .await;

    // Restart the node.
    // Use the hash of the current highest complete block as the trusted hash.
    let mut config = stopped_node.config;
    config.node.trusted_hash = Some(*fixture.highest_complete_block().hash());
    fixture
        .add_node(stopped_node.secret_key, config, stopped_node.storage_dir)
        .await;

    // Create & sign deploy to reactivate the stopped node's bid.
    // The bid amount will make sure that the rejoining validator proposes soon after it rejoins.
    let mut deploy = Deploy::add_bid(
        fixture.chainspec.network_config.name.clone(),
        fixture.system_contract_hash(AUCTION),
        stopped_public_key.clone(),
        //by default, validators in this flow have an account balance of
        // 100_000_000_000_000_000u64
        99_000_000_000_000_000_u64.into(),
        10,
        Timestamp::now(),
        TimeDiff::from_seconds(60),
    );
    deploy.sign(&stopped_secret_key);
    let txn = Transaction::Deploy(deploy);
    let txn_hash = txn.hash();

    // Inject the transaction and run the network until executed.
    fixture.inject_transaction(txn).await;
    fixture
        .run_until_executed_transaction(&txn_hash, THIRTY_SECS)
        .await;

    // Ensure execution succeeded and that there is a Write transform for the bid's key.
    let bid_key = Key::BidAddr(BidAddr::from(stopped_public_key.clone()));
    fixture
        .successful_execution_transforms(&txn_hash)
        .iter()
        .find(|transform| match transform.kind() {
            TransformKindV2::Write(StoredValue::BidKind(bid_kind)) => {
                Key::from(bid_kind.bid_addr()) == bid_key
            }
            _ => false,
        })
        .expect("should have a write record for bid");

    // Wait until the auction delay passes, plus one era for a margin of error.
    fixture
        .run_until_consensus_in_era(
            (2 * fixture.chainspec.core_config.auction_delay + 6).into(),
            ONE_MIN,
        )
        .await;
}

async fn assert_network_shutdown_for_upgrade_with_stakes(initial_stakes: InitialStakes) {
    let mut fixture = TestFixture::new(initial_stakes, None).await;

    // An upgrade is scheduled for era 2, after the switch block in era 1 (height 2).
    fixture.schedule_upgrade_for_era_two().await;

    // Run until the nodes shut down for the upgrade.
    fixture
        .network
        .settle_on_exit(&mut fixture.rng, ExitCode::Success, ONE_MIN)
        .await;
}

#[tokio::test]
async fn nodes_should_have_enough_signatures_before_upgrade_with_equal_stake() {
    // Equal stake ensures that one node was able to learn about signatures created by the other, by
    // whatever means necessary (gossiping, broadcasting, fetching, etc.).
    let initial_stakes = InitialStakes::AllEqual {
        count: 2,
        stake: u128::MAX,
    };
    assert_network_shutdown_for_upgrade_with_stakes(initial_stakes).await;
}

#[tokio::test]
async fn nodes_should_have_enough_signatures_before_upgrade_with_one_dominant_stake() {
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 255]);
    assert_network_shutdown_for_upgrade_with_stakes(initial_stakes).await;
}

#[tokio::test]
async fn dont_upgrade_without_switch_block() {
    let initial_stakes = InitialStakes::Random { count: 2 };
    let mut fixture = TestFixture::new(initial_stakes, None).await;
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    eprintln!(
        "Running 'dont_upgrade_without_switch_block' test with rng={}",
        fixture.rng
    );

    // An upgrade is scheduled for era 2, after the switch block in era 1 (height 2).
    // We artificially delay the execution of that block.
    fixture.schedule_upgrade_for_era_two().await;
    for runner in fixture.network.runners_mut() {
        let mut exec_request_received = false;
        runner.reactor_mut().inner_mut().set_filter(move |event| {
            if let MainEvent::ContractRuntimeRequest(
                ContractRuntimeRequest::EnqueueBlockForExecution {
                    executable_block, ..
                },
            ) = &event
            {
                if executable_block.era_report.is_some()
                    && executable_block.era_id == ERA_ONE
                    && !exec_request_received
                {
                    info!("delaying {}", executable_block);
                    exec_request_received = true;
                    return Either::Left(
                        time::sleep(Duration::from_secs(10)).event(move |_| event),
                    );
                }
                info!("not delaying {}", executable_block);
            }
            Either::Right(event)
        });
    }

    // Run until the nodes shut down for the upgrade.
    fixture
        .network
        .settle_on_exit(&mut fixture.rng, ExitCode::Success, ONE_MIN)
        .await;

    // Verify that the switch block has been stored: Even though it was delayed the node didn't
    // restart before executing and storing it.
    for runner in fixture.network.nodes().values() {
        let header = runner
            .main_reactor()
            .storage()
            .read_block_header_by_height(2, false)
            .expect("failed to read from storage")
            .expect("missing switch block");
        assert_eq!(ERA_ONE, header.era_id(), "era should be 1");
        assert!(header.is_switch_block(), "header should be switch block");
    }
}

#[tokio::test]
async fn should_store_finalized_approvals() {
    // Set up a network with two nodes where node 0 (Alice) is effectively guaranteed to be the
    // proposer.
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);
    let mut fixture = TestFixture::new(initial_stakes, None).await;

    let alice_secret_key = Arc::clone(&fixture.node_contexts[0].secret_key);
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::clone(&fixture.node_contexts[1].secret_key);
    let charlie_secret_key = Arc::new(SecretKey::random(&mut fixture.rng)); // just for ordering testing purposes

    // Wait for all nodes to complete era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    // Submit a transaction.
    let mut transaction_alice_bob = Transaction::from(
        Deploy::random_valid_native_transfer_without_deps(&mut fixture.rng),
    );
    let mut transaction_alice_bob_charlie = transaction_alice_bob.clone();
    let mut transaction_bob_alice = transaction_alice_bob.clone();

    transaction_alice_bob.sign(&alice_secret_key);
    transaction_alice_bob.sign(&bob_secret_key);

    transaction_alice_bob_charlie.sign(&alice_secret_key);
    transaction_alice_bob_charlie.sign(&bob_secret_key);
    transaction_alice_bob_charlie.sign(&charlie_secret_key);

    transaction_bob_alice.sign(&bob_secret_key);
    transaction_bob_alice.sign(&alice_secret_key);

    // We will be testing the correct sequence of approvals against the transaction signed by Bob
    // and Alice.
    // The transaction signed by Alice and Bob should give the same ordering of approvals.
    let expected_approvals: Vec<_> = transaction_bob_alice.approvals().iter().cloned().collect();

    // We'll give the transaction signed by Alice, Bob and Charlie to Bob, so these will be his
    // original approvals. Save these for checks later.
    let bobs_original_approvals: Vec<_> = transaction_alice_bob_charlie
        .approvals()
        .iter()
        .cloned()
        .collect();
    assert_ne!(bobs_original_approvals, expected_approvals);

    let transaction_hash = transaction_alice_bob.hash();

    for runner in fixture.network.runners_mut() {
        let transaction = if runner.main_reactor().consensus().public_key() == &alice_public_key {
            // Alice will propose the transaction signed by Alice and Bob.
            transaction_alice_bob.clone()
        } else {
            // Bob will receive the transaction signed by Alice, Bob and Charlie.
            transaction_alice_bob_charlie.clone()
        };
        runner
            .process_injected_effects(|effect_builder| {
                effect_builder
                    .put_transaction_to_storage(transaction.clone())
                    .ignore()
            })
            .await;
        runner
            .process_injected_effects(|effect_builder| {
                effect_builder
                    .announce_new_transaction_accepted(Arc::new(transaction), Source::Client)
                    .ignore()
            })
            .await;
    }

    // Run until the transaction gets executed.
    let has_stored_exec_results = |nodes: &Nodes| {
        nodes.values().all(|runner| {
            let read = runner
                .main_reactor()
                .storage()
                .read_execution_result(&transaction_hash);
            read.is_some()
        })
    };
    fixture.run_until(has_stored_exec_results, ONE_MIN).await;

    // Check if the approvals agree.
    for runner in fixture.network.nodes().values() {
        let maybe_dwa = runner
            .main_reactor()
            .storage()
            .get_transaction_with_finalized_approvals_by_hash(&transaction_hash);
        let maybe_finalized_approvals = maybe_dwa
            .as_ref()
            .and_then(|dwa| dwa.1.clone())
            .map(|fa| fa.iter().cloned().collect());
        let maybe_original_approvals = maybe_dwa
            .as_ref()
            .map(|(transaction, _approvals)| transaction.approvals().iter().cloned().collect());
        if runner.main_reactor().consensus().public_key() != &alice_public_key {
            // Bob should have finalized approvals, and his original approvals should be different.
            assert_eq!(
                maybe_finalized_approvals.as_ref(),
                Some(&expected_approvals)
            );
            assert_eq!(
                maybe_original_approvals.as_ref(),
                Some(&bobs_original_approvals)
            );
        } else {
            // Alice should only have the correct approvals as the original ones, and no finalized
            // approvals (as they wouldn't be stored, because they would be the same as the
            // original ones).
            assert_eq!(maybe_finalized_approvals.as_ref(), None);
            assert_eq!(maybe_original_approvals.as_ref(), Some(&expected_approvals));
        }
    }
}

#[tokio::test]
async fn should_update_last_progress_after_block_execution() {
    // Set up a network with two nodes.
    let initial_stakes = InitialStakes::FromVec(vec![u128::MAX, 1]);
    let mut fixture = TestFixture::new(initial_stakes, None).await;

    // Let all nodes reach consensus in era 0.
    fixture.run_until_consensus_in_era(ERA_ONE, ONE_MIN).await;

    // Prepare and submit a transaction.
    let transaction = Transaction::from(Deploy::random_valid_native_transfer_without_deps(
        &mut fixture.rng,
    ));
    let transaction_hash = transaction.hash();

    for runner in fixture.network.runners_mut() {
        let transaction = transaction.clone();
        runner
            .process_injected_effects(|eff| {
                eff.put_transaction_to_storage(transaction.clone()).ignore()
            })
            .await;

        runner
            .process_injected_effects(|eff| {
                eff.announce_new_transaction_accepted(Arc::new(transaction), Source::Client)
                    .ignore()
            })
            .await;
    }

    // For each node, capture its last_progress before execution.
    let stored_last_progresses: Vec<_> = fixture
        .network
        .nodes()
        .values()
        .map(|node| {
            let reactor = node.main_reactor();
            assert_eq!(reactor.state, ReactorState::Validate);
            reactor.last_progress
        })
        .collect();

    // Run until the transaction gets executed.
    let has_stored_exec_results = |nodes: &Nodes| {
        nodes.values().all(|runner| {
            let read = runner
                .main_reactor()
                .storage()
                .read_execution_result(&transaction_hash);
            read.is_some()
        })
    };
    fixture.run_until(has_stored_exec_results, ONE_MIN).await;

    // For each node, verify its last_progress has been updated.
    for (stored_last_progress, node) in stored_last_progresses
        .into_iter()
        .zip(fixture.network.nodes().values())
    {
        assert!(node.main_reactor().last_progress > stored_last_progress);
    }
}
