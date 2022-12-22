use std::{collections::BTreeMap, iter, sync::Arc, time::Duration};

use either::Either;
use num::Zero;
use num_rational::Ratio;
use rand::Rng;
use tempfile::TempDir;
use tokio::time;
use tracing::{error, info};

use casper_execution_engine::core::engine_state::GetBidsRequest;
use casper_types::{
    system::auction::{Bids, DelegationRate},
    testing::TestRng,
    EraId, Motes, ProtocolVersion, PublicKey, SecretKey, TimeDiff, Timestamp, U512,
};

use crate::{
    components::{
        consensus::{ClContext, ConsensusMessage, HighwayMessage, HighwayVertex},
        gossiper, network, storage,
        upgrade_watcher::NextUpgrade,
    },
    effect::{
        incoming::ConsensusMessageIncoming,
        requests::{ContractRuntimeRequest, DeployBufferRequest, NetworkRequest},
        EffectExt,
    },
    protocol::Message,
    reactor::{
        main_reactor::{Config, MainEvent, MainReactor},
        Runner,
    },
    testing::{
        self, filter_reactor::FilterReactor, network::TestingNetwork, ConditionCheckReactor,
    },
    types::{
        chainspec::{AccountConfig, AccountsConfig, ValidatorConfig},
        ActivationPoint, BlockHeader, Chainspec, ChainspecRawBytes, Deploy, ExitCode, NodeRng,
    },
    utils::{External, Loadable, Source, RESOURCES_PATH},
    WithDir,
};

struct TestChain {
    // Keys that validator instances will use, can include duplicates
    keys: Vec<Arc<SecretKey>>,
    storages: Vec<TempDir>,
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,
}

type Nodes = testing::network::Nodes<FilterReactor<MainReactor>>;

impl Runner<ConditionCheckReactor<FilterReactor<MainReactor>>> {
    fn main_reactor(&self) -> &MainReactor {
        self.reactor().inner().inner()
    }
}

impl TestChain {
    /// Instantiates a new test chain configuration.
    ///
    /// Generates secret keys for `size` validators and creates a matching chainspec.
    fn new(rng: &mut TestRng, size: usize) -> Self {
        let keys: Vec<Arc<SecretKey>> = (0..size)
            .map(|_| Arc::new(SecretKey::random(rng)))
            .collect();
        let stakes = keys
            .iter()
            .map(|secret_key| {
                // We use very large stakes so we would catch overflow issues.
                let stake = U512::from(rng.gen_range(100..999)) * U512::from(u128::MAX);
                let secret_key = secret_key.clone();
                (PublicKey::from(&*secret_key), stake)
            })
            .collect();
        Self::new_with_keys(rng, keys, stakes)
    }

    /// Instantiates a new test chain configuration.
    ///
    /// Takes a vector of bonded keys with specified bond amounts.
    fn new_with_keys(
        rng: &mut TestRng,
        keys: Vec<Arc<SecretKey>>,
        stakes: BTreeMap<PublicKey, U512>,
    ) -> Self {
        // Load the `local` chainspec.
        let (mut chainspec, chainspec_raw_bytes) =
            <(Chainspec, ChainspecRawBytes)>::from_resources("local");

        // Override accounts with those generated from the keys.
        let accounts = stakes
            .into_iter()
            .map(|(public_key, bonded_amount)| {
                let validator_config =
                    ValidatorConfig::new(Motes::new(bonded_amount), DelegationRate::zero());
                AccountConfig::new(
                    public_key,
                    Motes::new(U512::from(rng.gen_range(10000..99999999))),
                    Some(validator_config),
                )
            })
            .collect();
        let delegators = vec![];
        chainspec.network_config.accounts_config = AccountsConfig::new(accounts, delegators);

        // Make the genesis timestamp 60 seconds from now, to allow for all validators to start up.
        let genesis_time = Timestamp::now() + TimeDiff::from_seconds(60);
        info!(
            "creating test chain configuration, genesis: {}",
            genesis_time
        );
        chainspec.protocol_config.activation_point = ActivationPoint::Genesis(genesis_time);

        chainspec.core_config.minimum_era_height = 1;
        chainspec.core_config.finality_threshold_fraction = Ratio::new(34, 100);
        chainspec.core_config.era_duration = TimeDiff::from_millis(10);
        chainspec.core_config.auction_delay = 1;
        chainspec.core_config.unbonding_delay = 3;

        TestChain {
            keys,
            storages: Vec::new(),
            chainspec: Arc::new(chainspec),
            chainspec_raw_bytes: Arc::new(chainspec_raw_bytes),
        }
    }

    fn chainspec_mut(&mut self) -> &mut Chainspec {
        Arc::get_mut(&mut self.chainspec).unwrap()
    }

    /// Creates an initializer/validator configuration for the `idx`th validator.
    fn create_node_config(&mut self, idx: usize, first_node_port: u16) -> Config {
        // Set the network configuration.
        let mut cfg = Config {
            network: if idx == 0 {
                network::Config::default_local_net_first_node(first_node_port)
            } else {
                network::Config::default_local_net(first_node_port)
            },
            gossip: gossiper::Config::new_with_small_timeouts(),
            ..Default::default()
        };

        // Additionally set up storage in a temporary directory.
        let (storage_cfg, temp_dir) = storage::Config::default_for_tests();
        // ...and the secret key for our validator.
        {
            let secret_key_path = temp_dir.path().join("secret_key");
            self.keys[idx]
                .to_file(secret_key_path.clone())
                .expect("could not write secret key");
            cfg.consensus.secret_key_path = External::Path(secret_key_path);
        }
        self.storages.push(temp_dir);
        cfg.storage = storage_cfg;
        cfg
    }

    async fn create_initialized_network(
        &mut self,
        rng: &mut NodeRng,
    ) -> anyhow::Result<TestingNetwork<FilterReactor<MainReactor>>> {
        let root = RESOURCES_PATH.join("local");

        let mut network: TestingNetwork<FilterReactor<MainReactor>> = TestingNetwork::new();
        let first_node_port = testing::unused_port_on_localhost();

        for idx in 0..self.keys.len() {
            info!("creating node {}", idx);
            let cfg = self.create_node_config(idx, first_node_port);
            network
                .add_node_with_config_and_chainspec(
                    WithDir::new(root.clone(), cfg),
                    Arc::clone(&self.chainspec),
                    Arc::clone(&self.chainspec_raw_bytes),
                    rng,
                )
                .await
                .expect("could not add node to reactor");
        }

        Ok(network)
    }
}

/// Given an era number, returns a predicate to check if all of the nodes are in the specified era.
fn is_in_era(era_id: EraId) -> impl Fn(&Nodes) -> bool {
    move |nodes: &Nodes| {
        nodes
            .values()
            .all(|runner| runner.main_reactor().consensus().current_era() == Some(era_id))
    }
}

/// Given an era number, returns a predicate to check if all of the nodes have completed the
/// specified era.
fn has_completed_era(era_id: EraId) -> impl Fn(&Nodes) -> bool {
    move |nodes: &Nodes| {
        nodes.values().all(|runner| {
            runner
                .main_reactor()
                .storage()
                .read_highest_switch_block_headers(1)
                .unwrap()
                .last()
                .map_or(false, |header| header.era_id() == era_id)
        })
    }
}

fn is_ping(event: &MainEvent) -> bool {
    if let MainEvent::ConsensusMessageIncoming(ConsensusMessageIncoming {
        message: ConsensusMessage::Protocol { payload, .. },
        ..
    }) = event
    {
        return matches!(
            payload.clone().try_into_highway(),
            Ok(HighwayMessage::<ClContext>::NewVertex(HighwayVertex::Ping(
                _
            )))
        );
    }
    false
}

/// A set of consecutive switch blocks.
struct SwitchBlocks {
    headers: Vec<BlockHeader>,
}

impl SwitchBlocks {
    /// Collects all switch blocks of the first `era_count` eras, and asserts that they are equal
    /// in all nodes.
    fn collect(nodes: &Nodes, era_count: u64) -> SwitchBlocks {
        let mut headers = Vec::new();
        for era_number in 0..era_count {
            let mut header_iter = nodes.values().map(|runner| {
                let storage = runner.main_reactor().storage();
                let maybe_block = storage
                    .transactional_get_switch_block_by_era_id(era_number)
                    .expect("failed to get switch block by era id");
                maybe_block.expect("missing switch block").take_header()
            });
            let header = header_iter.next().unwrap();
            assert_eq!(era_number, header.era_id().value());
            for other_header in header_iter {
                assert_eq!(header, other_header);
            }
            headers.push(header);
        }
        SwitchBlocks { headers }
    }

    /// Returns the list of equivocators in the given era.
    fn equivocators(&self, era_number: u64) -> &[PublicKey] {
        &self.headers[era_number as usize]
            .era_end()
            .expect("era end")
            .era_report()
            .equivocators
    }

    /// Returns the list of inactive validators in the given era.
    fn inactive_validators(&self, era_number: u64) -> &[PublicKey] {
        &self.headers[era_number as usize]
            .era_end()
            .expect("era end")
            .era_report()
            .inactive_validators
    }

    /// Returns the list of validators in the successor era.
    fn next_era_validators(&self, era_number: u64) -> &BTreeMap<PublicKey, U512> {
        self.headers[era_number as usize]
            .next_era_validator_weights()
            .expect("validators")
    }

    /// Returns the set of bids in the auction contract at the end of the given era.
    fn bids(&self, nodes: &Nodes, era_number: u64) -> Bids {
        let correlation_id = Default::default();
        let state_root_hash = *self.headers[era_number as usize].state_root_hash();
        for runner in nodes.values() {
            let request = GetBidsRequest::new(state_root_hash);
            let engine_state = runner.main_reactor().contract_runtime().engine_state();
            let bids_result = engine_state
                .get_bids(correlation_id, request)
                .expect("get_bids failed");
            if let Some(bids) = bids_result.into_success() {
                return bids;
            }
        }
        unreachable!("at least one node should have bids for era {}", era_number);
    }
}

#[tokio::test]
async fn run_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    // Instantiate a new chain with a fixed size.
    const NETWORK_SIZE: usize = 5;
    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE);

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    // Wait for all nodes to agree on one era.
    net.settle_on(
        &mut rng,
        is_in_era(EraId::from(1)),
        Duration::from_secs(1000),
    )
    .await;

    net.settle_on(
        &mut rng,
        is_in_era(EraId::from(2)),
        Duration::from_secs(1001),
    )
    .await;
}

#[tokio::test]
async fn run_equivocator_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    let alice_secret_key = Arc::new(SecretKey::random(&mut rng));
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::new(SecretKey::random(&mut rng));
    let bob_public_key = PublicKey::from(&*bob_secret_key);

    let size: usize = 3;
    // Leave two free slots for Alice and Bob.
    let mut keys: Vec<Arc<SecretKey>> = (2..size)
        .map(|_| Arc::new(SecretKey::random(&mut rng)))
        .collect();
    let mut stakes: BTreeMap<PublicKey, U512> = keys
        .iter()
        .map(|secret_key| (PublicKey::from(&*secret_key.clone()), U512::from(100000u64)))
        .collect();
    stakes.insert(PublicKey::from(&*alice_secret_key), U512::from(1));
    stakes.insert(PublicKey::from(&*bob_secret_key), U512::from(1));

    // Here's where things go wrong: Bob doesn't run a node at all, and Alice runs two!
    keys.push(alice_secret_key.clone());
    keys.push(alice_secret_key);

    // We configure the era to take ten rounds. That should guarantee that the two nodes
    // equivocate.
    let mut chain = TestChain::new_with_keys(&mut rng, keys, stakes.clone());
    chain.chainspec_mut().core_config.minimum_era_height = 10;
    chain.chainspec_mut().highway_config.maximum_round_length =
        chain.chainspec.core_config.minimum_block_time * 2;
    chain.chainspec_mut().core_config.validator_slots = size as u32;

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");
    let min_round_len = chain.chainspec.core_config.minimum_block_time;
    let mut maybe_first_message_time = None;

    let mut alice_reactors = net
        .reactors_mut()
        .filter(|reactor| *reactor.inner().consensus().public_key() == alice_public_key);

    // Delay all messages to and from the first of Alice's nodes until three rounds after the first
    // message.  Further, significantly delay any incoming pings to avoid the node detecting the
    // doppelganger and deactivating itself.
    alice_reactors.next().unwrap().set_filter(move |event| {
        if is_ping(&event) {
            return Either::Left(time::sleep((min_round_len * 30).into()).event(move |_| event));
        }
        let now = Timestamp::now();
        match &event {
            MainEvent::ConsensusMessageIncoming(_) => {}
            MainEvent::NetworkRequest(
                NetworkRequest::SendMessage { payload, .. }
                | NetworkRequest::ValidatorBroadcast { payload, .. }
                | NetworkRequest::Gossip { payload, .. },
            ) if matches!(**payload, Message::Consensus(_)) => {}
            _ => return Either::Right(event),
        };
        let first_message_time = *maybe_first_message_time.get_or_insert(now);
        if now < first_message_time + min_round_len * 3 {
            return Either::Left(time::sleep(min_round_len.into()).event(move |_| event));
        }
        Either::Right(event)
    });

    // Significantly delay all incoming pings to the second of Alice's nodes.
    alice_reactors.next().unwrap().set_filter(move |event| {
        if is_ping(&event) {
            return Either::Left(time::sleep((min_round_len * 30).into()).event(move |_| event));
        }
        Either::Right(event)
    });

    drop(alice_reactors);

    let era_count = 4;

    let timeout = Duration::from_secs(90 * era_count);
    info!("Waiting for {} eras to end.", era_count);
    net.settle_on(
        &mut rng,
        has_completed_era(EraId::new(era_count - 1)),
        timeout,
    )
    .await;
    let switch_blocks = SwitchBlocks::collect(net.nodes(), era_count);
    let bids: Vec<Bids> = (0..era_count)
        .map(|era_number| switch_blocks.bids(net.nodes(), era_number))
        .collect();

    // Since this setup sometimes produces an equivocation in era 2 rather than era 1, we set an
    // offset here.
    // TODO: Remove this once https://github.com/casper-network/casper-node/issues/1859 is fixed.
    let offset = if switch_blocks.equivocators(1).is_empty() {
        error!("failed to equivocate in era 1 - asserting equivocation detected in era 2");
        1
    } else {
        0
    };

    // Era 0 consists only of the genesis block.
    // In era 1, Alice equivocates. Since eviction takes place with a delay of one
    // (`auction_delay`) era, she is still included in the next era's validator set.
    assert_eq!(
        switch_blocks.equivocators(1 + offset),
        [alice_public_key.clone()]
    );
    assert!(bids[1 + offset as usize][&alice_public_key].inactive());
    assert!(switch_blocks
        .next_era_validators(1 + offset)
        .contains_key(&alice_public_key));

    // In era 2 Alice is banned. Banned validators count neither as faulty nor inactive, even
    // though they cannot participate. In the next era, she will be evicted.
    assert_eq!(switch_blocks.equivocators(2 + offset), []);
    assert!(bids[2 + offset as usize][&alice_public_key].inactive());
    assert!(!switch_blocks
        .next_era_validators(2 + offset)
        .contains_key(&alice_public_key));

    // In era 3 she is not a validator anymore and her bid remains deactivated.
    if offset == 0 {
        assert_eq!(switch_blocks.equivocators(3), []);
        assert!(bids[3][&alice_public_key].inactive());
        assert!(!switch_blocks
            .next_era_validators(3)
            .contains_key(&alice_public_key));
    }

    // Bob is inactive.
    assert_eq!(
        switch_blocks.inactive_validators(1),
        [bob_public_key.clone()]
    );
    assert_eq!(
        switch_blocks.inactive_validators(2),
        [bob_public_key.clone()]
    );

    // We don't slash, so the stakes are never reduced.
    for (public_key, stake) in &stakes {
        assert!(bids[0][public_key].staked_amount() >= stake);
        assert!(bids[1][public_key].staked_amount() >= stake);
        assert!(bids[2][public_key].staked_amount() >= stake);
        assert!(bids[3][public_key].staked_amount() >= stake);
    }
}

#[tokio::test]
async fn dont_upgrade_without_switch_block() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    const NETWORK_SIZE: usize = 2;
    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE);
    chain.chainspec_mut().core_config.minimum_era_height = 2;
    chain.chainspec_mut().core_config.era_duration = TimeDiff::from_millis(0);
    chain.chainspec_mut().core_config.minimum_block_time = "1second".parse().unwrap();

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    // An upgrade is scheduled for era 2, after the switch block in era 1 (height 2).
    // We artificially delay the execution of that block.
    for runner in net.runners_mut() {
        runner
            .process_injected_effects(|effect_builder| {
                let upgrade = NextUpgrade::new(
                    ActivationPoint::EraId(2.into()),
                    ProtocolVersion::from_parts(999, 0, 0),
                );
                effect_builder
                    .announce_upgrade_activation_point_read(upgrade)
                    .ignore()
            })
            .await;
        let mut exec_request_received = false;
        runner.reactor_mut().inner_mut().set_filter(move |event| {
            if let MainEvent::ContractRuntimeRequest(
                ContractRuntimeRequest::EnqueueBlockForExecution {
                    finalized_block, ..
                },
            ) = &event
            {
                if finalized_block.era_report().is_some()
                    && finalized_block.era_id() == EraId::from(1)
                    && !exec_request_received
                {
                    info!("delaying {}", finalized_block);
                    exec_request_received = true;
                    return Either::Left(
                        time::sleep(Duration::from_secs(10)).event(move |_| event),
                    );
                }
                info!("not delaying {}", finalized_block);
            }
            Either::Right(event)
        });
    }

    // Run until the nodes shut down for the upgrade.
    let timeout = Duration::from_secs(90);
    net.settle_on_exit(&mut rng, ExitCode::Success, timeout)
        .await;

    // Verify that the switch block has been stored: Even though it was delayed the node didn't
    // restart before executing and storing it.
    for runner in net.nodes().values() {
        let header = runner
            .main_reactor()
            .storage()
            .read_block_by_height(2)
            .expect("failed to read from storage")
            .expect("missing switch block")
            .take_header();
        assert_eq!(EraId::from(1), header.era_id());
        assert!(header.is_switch_block());
    }
}

#[tokio::test]
async fn should_store_finalized_approvals() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    // Set up a network with two nodes.
    let alice_secret_key = Arc::new(SecretKey::random(&mut rng));
    let alice_public_key = PublicKey::from(&*alice_secret_key);
    let bob_secret_key = Arc::new(SecretKey::random(&mut rng));
    let charlie_secret_key = Arc::new(SecretKey::random(&mut rng)); // just for ordering testing purposes
    let keys: Vec<Arc<SecretKey>> = vec![alice_secret_key.clone(), bob_secret_key.clone()];
    // only Alice will be proposing blocks
    let stakes: BTreeMap<PublicKey, U512> =
        iter::once((alice_public_key.clone(), U512::from(100))).collect();

    // Eras have exactly two blocks each, and there is one block per second.
    let mut chain = TestChain::new_with_keys(&mut rng, keys, stakes.clone());
    chain.chainspec_mut().core_config.minimum_era_height = 2;
    chain.chainspec_mut().core_config.era_duration = TimeDiff::from_millis(0);
    chain.chainspec_mut().core_config.minimum_block_time = "1second".parse().unwrap();
    chain.chainspec_mut().core_config.validator_slots = 1;

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    // Wait for all nodes to complete era 0.
    net.settle_on(
        &mut rng,
        has_completed_era(EraId::from(0)),
        Duration::from_secs(90),
    )
    .await;

    // Submit a deploy.
    let mut deploy_alice_bob = Deploy::random_valid_native_transfer_without_deps(&mut rng);
    let mut deploy_alice_bob_charlie = deploy_alice_bob.clone();
    let mut deploy_bob_alice = deploy_alice_bob.clone();

    deploy_alice_bob.sign(&*alice_secret_key);
    deploy_alice_bob.sign(&*bob_secret_key);

    deploy_alice_bob_charlie.sign(&*alice_secret_key);
    deploy_alice_bob_charlie.sign(&*bob_secret_key);
    deploy_alice_bob_charlie.sign(&*charlie_secret_key);

    deploy_bob_alice.sign(&*bob_secret_key);
    deploy_bob_alice.sign(&*alice_secret_key);

    // We will be testing the correct sequence of approvals against the deploy signed by Bob and
    // Alice.
    // The deploy signed by Alice and Bob should give the same ordering of approvals.
    let expected_approvals: Vec<_> = deploy_bob_alice.approvals().iter().cloned().collect();

    // We'll give the deploy signed by Alice, Bob and Charlie to Bob, so these will be his original
    // approvals. Save these for checks later.
    let bobs_original_approvals: Vec<_> = deploy_alice_bob_charlie
        .approvals()
        .iter()
        .cloned()
        .collect();
    assert_ne!(bobs_original_approvals, expected_approvals);

    let deploy_hash = *deploy_alice_bob.deploy_or_transfer_hash().deploy_hash();

    for runner in net.runners_mut() {
        if runner.main_reactor().consensus().public_key() == &alice_public_key {
            // Alice will propose the deploy signed by Alice and Bob.
            runner
                .process_injected_effects(|effect_builder| {
                    effect_builder
                        .put_deploy_to_storage(Box::new(deploy_alice_bob.clone()))
                        .ignore()
                })
                .await;
            runner
                .process_injected_effects(|effect_builder| {
                    effect_builder
                        .announce_new_deploy_accepted(
                            Box::new(deploy_alice_bob.clone()),
                            Source::Client,
                        )
                        .ignore()
                })
                .await;
        } else {
            // Bob will receive the deploy signed by Alice, Bob and Charlie.
            runner
                .process_injected_effects(|effect_builder| {
                    effect_builder
                        .put_deploy_to_storage(Box::new(deploy_alice_bob_charlie.clone()))
                        .ignore()
                })
                .await;
            runner
                .process_injected_effects(|effect_builder| {
                    effect_builder
                        .announce_new_deploy_accepted(
                            Box::new(deploy_alice_bob_charlie.clone()),
                            Source::Client,
                        )
                        .ignore()
                })
                .await;
        }
    }

    // Run until the deploy gets executed.
    let timeout = Duration::from_secs(90);
    net.settle_on(
        &mut rng,
        |nodes| {
            nodes.values().all(|runner| {
                runner
                    .main_reactor()
                    .storage()
                    .get_deploy_metadata_by_hash(&deploy_hash)
                    .is_some()
            })
        },
        timeout,
    )
    .await;

    // Check if the approvals agree.
    for runner in net.nodes().values() {
        let maybe_dwa = runner
            .main_reactor()
            .storage()
            .get_deploy_with_finalized_approvals_by_hash(&deploy_hash);
        let maybe_finalized_approvals = maybe_dwa
            .as_ref()
            .and_then(|dwa| dwa.finalized_approvals())
            .map(|fa| fa.inner().iter().cloned().collect());
        let maybe_original_approvals = maybe_dwa
            .as_ref()
            .map(|dwa| dwa.original_approvals().iter().cloned().collect());
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
async fn empty_block_validation_regression() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    let size: usize = 4;
    let keys: Vec<Arc<SecretKey>> = (0..size)
        .map(|_| Arc::new(SecretKey::random(&mut rng)))
        .collect();
    let stakes: BTreeMap<PublicKey, U512> = keys
        .iter()
        .map(|secret_key| (PublicKey::from(&*secret_key.clone()), U512::from(100u64)))
        .collect();

    // We make the first validator always accuse everyone else.
    let mut chain = TestChain::new_with_keys(&mut rng, keys, stakes.clone());
    chain.chainspec_mut().core_config.minimum_block_time = "1second".parse().unwrap();
    chain.chainspec_mut().highway_config.maximum_round_length = "1second".parse().unwrap();
    chain.chainspec_mut().core_config.minimum_era_height = 15;
    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");
    let malicious_validator = stakes.keys().next().unwrap().clone();
    info!("Malicious validator: {:?}", malicious_validator);
    let _everyone_else: Vec<_> = stakes
        .keys()
        .filter(|pub_key| **pub_key != malicious_validator)
        .cloned()
        .collect();
    let malicious_runner = net
        .runners_mut()
        .find(|runner| runner.main_reactor().consensus().public_key() == &malicious_validator)
        .unwrap();
    malicious_runner
        .reactor_mut()
        .inner_mut()
        .set_filter(move |event| match event {
            MainEvent::DeployBufferRequest(DeployBufferRequest::GetAppendableBlock {
                timestamp,
                responder,
            }) => {
                info!("Accusing everyone else!");
                Either::Right(MainEvent::DeployBufferRequest(
                    DeployBufferRequest::GetAppendableBlock {
                        timestamp,
                        responder,
                    },
                ))
            }
            event => Either::Right(event),
        });

    let timeout = Duration::from_secs(300);
    info!("Waiting for the first era to end.");
    net.settle_on(&mut rng, is_in_era(EraId::new(1)), timeout)
        .await;
    let switch_blocks = SwitchBlocks::collect(net.nodes(), 1);

    // Nobody actually double-signed. The accusations should have had no effect.
    assert_eq!(switch_blocks.equivocators(0), []);
    // If the malicious validator was the first proposer, all their Highway units might be invalid,
    // because they all refer to the invalid proposal, so they might get flagged as inactive. No
    // other validators should be considered inactive.
    match switch_blocks.inactive_validators(0) {
        [] => {}
        [inactive_validator] if malicious_validator == *inactive_validator => {}
        inactive => panic!("unexpected inactive validators: {:?}", inactive),
    }
}
