use std::{collections::BTreeMap, iter, sync::Arc, time::Duration};

use either::Either;
use num::Zero;
use num_rational::Ratio;
use rand::Rng;
use tempfile::TempDir;
use tokio::time;
use tracing::{error, info};

use casper_execution_engine::engine_state::{
    GetBidsRequest, GetBidsResult, SystemContractRegistry,
};
use casper_storage::global_state::state::{StateProvider, StateReader};
use casper_types::{
    execution::{Effects, ExecutionResult, ExecutionResultV2, TransformKind},
    system::auction::{BidAddr, BidKind, BidsExt, DelegationRate},
    testing::TestRng,
    AccountConfig, AccountsConfig, ActivationPoint, Block, BlockHash, BlockHeader, CLValue,
    Chainspec, ChainspecRawBytes, ContractHash, Deploy, DeployHash, EraId, Key, Motes,
    ProtocolVersion, PublicKey, SecretKey, StoredValue, TimeDiff, Timestamp, Transaction,
    TransactionHash, ValidatorConfig, U512,
};

use crate::{
    components::{
        consensus::{
            self, ClContext, ConsensusMessage, HighwayMessage, HighwayVertex, NewBlockPayload,
        },
        gossiper, network, storage,
        upgrade_watcher::NextUpgrade,
    },
    effect::{
        incoming::ConsensusMessageIncoming,
        requests::{ContractRuntimeRequest, NetworkRequest},
        EffectExt,
    },
    protocol::Message,
    reactor::{
        main_reactor::{Config, MainEvent, MainReactor, ReactorState},
        Runner,
    },
    testing::{
        self, filter_reactor::FilterReactor, network::TestingNetwork, ConditionCheckReactor,
    },
    types::{
        AvailableBlockRange, BlockPayload, DeployOrTransferHash, DeployWithFinalizedApprovals,
        ExitCode, NodeId, NodeRng, SyncHandling, TransactionWithFinalizedApprovals,
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
    first_node_port: u16,
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
    fn new(rng: &mut TestRng, size: usize, initial_stakes: Option<&[U512]>) -> Self {
        let keys: Vec<Arc<SecretKey>> = (0..size)
            .map(|_| Arc::new(SecretKey::random(rng)))
            .collect();

        let stake_values = if let Some(initial_stakes) = initial_stakes {
            assert_eq!(size, initial_stakes.len());
            initial_stakes.to_vec()
        } else {
            // By default we use very large stakes so we would catch overflow issues.
            iter::from_fn(|| Some(U512::from(rng.gen_range(100..999)) * U512::from(u128::MAX)))
                .take(size)
                .collect()
        };

        let stakes = keys
            .iter()
            .zip(stake_values)
            .map(|(secret_key, stake)| {
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

        let min_motes = 100_000_000_000u64; // 1000 token
        let max_motes = min_motes * 100; // 100_000 token
        let balance = U512::from(rng.gen_range(min_motes..max_motes));

        // Override accounts with those generated from the keys.
        let accounts = stakes
            .into_iter()
            .map(|(public_key, bonded_amount)| {
                let validator_config =
                    ValidatorConfig::new(Motes::new(bonded_amount), DelegationRate::zero());
                AccountConfig::new(public_key, Motes::new(balance), Some(validator_config))
            })
            .collect();
        let delegators = vec![];
        let administrators = vec![];
        chainspec.network_config.accounts_config =
            AccountsConfig::new(accounts, delegators, administrators);

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

        let first_node_port = testing::unused_port_on_localhost();

        TestChain {
            keys,
            storages: Vec::new(),
            chainspec: Arc::new(chainspec),
            chainspec_raw_bytes: Arc::new(chainspec_raw_bytes),
            first_node_port,
        }
    }

    fn chainspec_mut(&mut self) -> &mut Chainspec {
        Arc::get_mut(&mut self.chainspec).unwrap()
    }

    fn chainspec(&self) -> Arc<Chainspec> {
        self.chainspec.clone()
    }

    fn chainspec_raw_bytes(&self) -> Arc<ChainspecRawBytes> {
        self.chainspec_raw_bytes.clone()
    }

    fn first_node_port(&self) -> u16 {
        self.first_node_port
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

        for idx in 0..self.keys.len() {
            info!("creating node {}", idx);
            let cfg = self.create_node_config(idx, self.first_node_port);
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

fn available_block_range(
    runner: &Runner<ConditionCheckReactor<FilterReactor<MainReactor>>>,
) -> AvailableBlockRange {
    let storage = runner.main_reactor().storage();
    storage.get_available_block_range()
}

/// Given a block height and a node id, returns a predicate to check if the lowest available block
/// for the specified node is at or below the specified height.
fn node_has_lowest_available_block_at_or_below_height(
    height: u64,
    node_id: NodeId,
) -> impl Fn(&Nodes) -> bool {
    move |nodes: &Nodes| {
        nodes.get(&node_id).map_or(true, |runner| {
            let available_block_range = available_block_range(runner);
            if available_block_range.low() == 0 && available_block_range.high() == 0 {
                false
            } else {
                available_block_range.low() <= height
            }
        })
    }
}

fn highest_complete_block(
    runner: &Runner<ConditionCheckReactor<FilterReactor<MainReactor>>>,
) -> Option<Block> {
    let storage = runner.main_reactor().storage();
    storage.read_highest_complete_block().unwrap_or(None)
}

fn highest_complete_block_hash(
    runner: &Runner<ConditionCheckReactor<FilterReactor<MainReactor>>>,
) -> Option<BlockHash> {
    highest_complete_block(runner).map(|block| *block.hash())
}

fn is_ping(event: &MainEvent) -> bool {
    if let MainEvent::ConsensusMessageIncoming(ConsensusMessageIncoming { message, .. }) = event {
        if let ConsensusMessage::Protocol { ref payload, .. } = **message {
            return matches!(
                payload.deserialize_incoming::<HighwayMessage::<ClContext>>(),
                Ok(HighwayMessage::<ClContext>::NewVertex(HighwayVertex::Ping(
                    _
                )))
            );
        }
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
                    .read_switch_block_by_era_id(EraId::from(era_number))
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
        self.headers[era_number as usize]
            .maybe_equivocators()
            .expect("era end")
    }

    /// Returns the list of inactive validators in the given era.
    fn inactive_validators(&self, era_number: u64) -> &[PublicKey] {
        self.headers[era_number as usize]
            .maybe_inactive_validators()
            .expect("era end")
    }

    /// Returns the list of validators in the successor era.
    fn next_era_validators(&self, era_number: u64) -> &BTreeMap<PublicKey, U512> {
        self.headers[era_number as usize]
            .next_era_validator_weights()
            .expect("validators")
    }

    /// Returns the set of bids in the auction contract at the end of the given era.
    fn bids(&self, nodes: &Nodes, era_number: u64) -> Vec<BidKind> {
        let state_root_hash = *self.headers[era_number as usize].state_root_hash();
        for runner in nodes.values() {
            let request = GetBidsRequest::new(state_root_hash);
            let engine_state = runner.main_reactor().contract_runtime().engine_state();
            let bids_result = engine_state.get_bids(request).expect("get_bids failed");
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
    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE, None);

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
async fn historical_sync_with_era_height_1() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    // Instantiate a new chain with a fixed size.
    const NETWORK_SIZE: usize = 5;
    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE, None);
    chain.chainspec_mut().core_config.minimum_era_height = 1;

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    // Wait for all nodes to reach era 3.
    net.settle_on(
        &mut rng,
        is_in_era(EraId::from(3)),
        Duration::from_secs(1000),
    )
    .await;

    let (_, first_node) = net
        .nodes()
        .iter()
        .next()
        .expect("Expected non-empty network");

    // Get a trusted hash
    let lfb = highest_complete_block_hash(first_node)
        .expect("Could not determine the latest complete block for this network");

    // Create a joiner node
    let mut config = Config {
        network: network::Config::default_local_net(chain.first_node_port()),
        gossip: gossiper::Config::new_with_small_timeouts(),
        ..Default::default()
    };
    let joiner_key = Arc::new(SecretKey::random(&mut rng));
    let (storage_cfg, temp_dir) = storage::Config::default_for_tests();
    {
        let secret_key_path = temp_dir.path().join("secret_key");
        joiner_key
            .to_file(secret_key_path.clone())
            .expect("could not write secret key");
        config.consensus.secret_key_path = External::Path(secret_key_path);
    }
    config.storage = storage_cfg;
    config.node.trusted_hash = Some(lfb);
    config.node.sync_handling = SyncHandling::Genesis;
    let root = RESOURCES_PATH.join("local");
    let cfg = WithDir::new(root.clone(), config);

    let (joiner_id, _) = net
        .add_node_with_config_and_chainspec(
            cfg,
            chain.chainspec(),
            chain.chainspec_raw_bytes(),
            &mut rng,
        )
        .await
        .expect("could not add node to reactor");

    // Wait for joiner node to sync back to the block from era 1
    net.settle_on(
        &mut rng,
        node_has_lowest_available_block_at_or_below_height(1, joiner_id),
        Duration::from_secs(1000),
    )
    .await;

    // Remove the weights for era 0 and era 1 from the validator matrix
    let runner = net
        .nodes_mut()
        .get_mut(&joiner_id)
        .expect("Could not find runner for node {joiner_id}");
    let reactor = runner.reactor_mut().inner_mut().inner_mut();
    reactor
        .validator_matrix
        .purge_era_validators(&EraId::from(0));
    reactor
        .validator_matrix
        .purge_era_validators(&EraId::from(1));

    // Continue syncing and check if the joiner node reaches era 0
    net.settle_on(
        &mut rng,
        node_has_lowest_available_block_at_or_below_height(0, joiner_id),
        Duration::from_secs(1000),
    )
    .await;
}

#[tokio::test]
async fn should_not_historical_sync_no_sync_node() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    // Instantiate a new chain with a fixed size.
    const NETWORK_SIZE: usize = 5;
    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE, None);
    chain.chainspec_mut().core_config.minimum_era_height = 1;

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    // Wait for all nodes to reach era 1.
    net.settle_on(
        &mut rng,
        is_in_era(EraId::from(1)),
        Duration::from_secs(100),
    )
    .await;

    let (_, first_node) = net
        .nodes()
        .iter()
        .next()
        .expect("Expected non-empty network");

    let highest_block = highest_complete_block(first_node).expect("should have block");
    let trusted_hash = *highest_block.hash();
    let trusted_height = highest_block.height();

    // Create a joiner node
    let mut config = Config {
        network: network::Config::default_local_net(chain.first_node_port()),
        gossip: gossiper::Config::new_with_small_timeouts(),
        ..Default::default()
    };
    let joiner_key = Arc::new(SecretKey::random(&mut rng));
    let (storage_cfg, temp_dir) = storage::Config::default_for_tests();
    {
        let secret_key_path = temp_dir.path().join("secret_key");
        joiner_key
            .to_file(secret_key_path.clone())
            .expect("could not write secret key");
        config.consensus.secret_key_path = External::Path(secret_key_path);
    }
    config.storage = storage_cfg;
    config.node.trusted_hash = Some(trusted_hash);
    config.node.sync_handling = SyncHandling::NoSync;
    let root = RESOURCES_PATH.join("local");
    let cfg = WithDir::new(root.clone(), config);

    let (joiner_id, _) = net
        .add_node_with_config_and_chainspec(
            cfg,
            chain.chainspec(),
            chain.chainspec_raw_bytes(),
            &mut rng,
        )
        .await
        .expect("could not add node to reactor");

    // Wait for all nodes to reach era 2.
    net.settle_on(
        &mut rng,
        has_completed_era(EraId::from(2)),
        Duration::from_secs(100),
    )
    .await;

    let available_block_range_pre = {
        let (_, runner) = net
            .nodes_mut()
            .iter()
            .find(|(x, _)| *x == &joiner_id)
            .expect("should have runner");
        available_block_range(runner)
    };

    let pre = available_block_range_pre.low();
    assert!(
        pre >= trusted_height,
        "should not have acquired a block earlier than trusted hash block {} {}",
        pre,
        trusted_height
    );

    // Wait for all nodes to reach era 3.
    net.settle_on(
        &mut rng,
        has_completed_era(EraId::from(3)),
        Duration::from_secs(100),
    )
    .await;

    let available_block_range_post = {
        let (_, runner) = net
            .nodes_mut()
            .iter()
            .find(|(x, _)| *x == &joiner_id)
            .expect("should have runner 2");
        available_block_range(runner)
    };

    let post = available_block_range_post.low();

    assert!(
        pre <= post,
        "should not have acquired earlier blocks {} {}",
        pre,
        post
    );

    let pre = available_block_range_pre.high();
    let post = available_block_range_post.high();
    assert!(
        pre < post,
        "should have acquired later blocks {} {}",
        pre,
        post
    );
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

    let timeout = Duration::from_secs(120 * era_count);
    info!("Waiting for {} eras to end.", era_count);
    net.settle_on(
        &mut rng,
        has_completed_era(EraId::new(era_count - 1)),
        timeout,
    )
    .await;

    // network settled; select data to analyze
    let switch_blocks = SwitchBlocks::collect(net.nodes(), era_count);
    let mut era_bids = BTreeMap::new();
    for era in 0..era_count {
        era_bids.insert(era, switch_blocks.bids(net.nodes(), era));
    }

    // Since this setup sometimes produces no equivocation or an equivocation in era 2 rather than
    // era 1, we set an offset here.  If neither eras has an equivocation, exit early.
    // TODO: Remove this once https://github.com/casper-network/casper-node/issues/1859 is fixed.
    for switch_block in &switch_blocks.headers {
        let era_id = switch_block.era_id();
        let count = switch_blocks.equivocators(era_id.value()).len();
        info!("equivocators in {}: {}", era_id, count);
    }
    let offset = if !switch_blocks.equivocators(1).is_empty() {
        0
    } else if !switch_blocks.equivocators(2).is_empty() {
        error!("failed to equivocate in era 1 - asserting equivocation detected in era 2");
        1
    } else {
        error!("failed to equivocate in era 1");
        return;
    };

    // Era 0 consists only of the genesis block.
    // In era 1, Alice equivocates. Since eviction takes place with a delay of one
    // (`auction_delay`) era, she is still included in the next era's validator set.
    let next_era_id = 1 + offset;

    assert_eq!(
        switch_blocks.equivocators(next_era_id),
        [alice_public_key.clone()]
    );
    let next_era_bids = era_bids.get(&next_era_id).expect("should have offset era");

    let next_era_alice = next_era_bids
        .validator_bid(&alice_public_key)
        .expect("should have Alice's offset bid");
    assert!(
        next_era_alice.inactive(),
        "Alice's bid should be inactive in offset era."
    );
    assert!(switch_blocks
        .next_era_validators(next_era_id)
        .contains_key(&alice_public_key));

    // In era 2 Alice is banned. Banned validators count neither as faulty nor inactive, even
    // though they cannot participate. In the next era, she will be evicted.
    let future_era_id = 2 + offset;
    assert_eq!(switch_blocks.equivocators(future_era_id), []);
    let future_era_bids = era_bids
        .get(&future_era_id)
        .expect("should have future era");
    let future_era_alice = future_era_bids
        .validator_bid(&alice_public_key)
        .expect("should have Alice's future bid");
    assert!(
        future_era_alice.inactive(),
        "Alice's bid should be inactive in future era."
    );
    assert!(!switch_blocks
        .next_era_validators(future_era_id)
        .contains_key(&alice_public_key));

    // In era 3 Alice is not a validator anymore and her bid remains deactivated.
    let era_3 = 3;
    if offset == 0 {
        assert_eq!(switch_blocks.equivocators(era_3), []);
        let era_3_bids = era_bids.get(&era_3).expect("should have era 3 bids");
        let era_3_alice = era_3_bids
            .validator_bid(&alice_public_key)
            .expect("should have Alice's era 3 bid");
        assert!(
            era_3_alice.inactive(),
            "Alice's bid should be inactive in era 3."
        );
        assert!(!switch_blocks
            .next_era_validators(era_3)
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

    for (era, bids) in era_bids {
        for (public_key, stake) in &stakes {
            let bid = bids
                .validator_bid(public_key)
                .expect("should have bid for public key {public_key} in era {era}");
            let staked_amount = bid.staked_amount();
            assert!(
                staked_amount >= *stake,
                "expected stake {} for public key {} in era {}, found {}",
                staked_amount,
                public_key,
                era,
                stake
            );
        }
    }
}

async fn assert_network_shutdown_for_upgrade_with_stakes(rng: &mut TestRng, stakes: &[U512]) {
    const NETWORK_SIZE: usize = 2;
    const INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(20);

    let mut chain = TestChain::new(rng, NETWORK_SIZE, Some(stakes));
    chain.chainspec_mut().core_config.minimum_era_height = 2;
    chain.chainspec_mut().core_config.era_duration = TimeDiff::from_millis(0);
    chain.chainspec_mut().core_config.minimum_block_time = "1second".parse().unwrap();

    let mut net = chain
        .create_initialized_network(rng)
        .await
        .expect("network initialization failed");

    // Wait until initialization is finished, so upgrade watcher won't reject test requests.
    net.settle_on(
        rng,
        move |nodes: &Nodes| {
            nodes
                .values()
                .all(|runner| !matches!(runner.main_reactor().state, ReactorState::Initialize))
        },
        INITIALIZATION_TIMEOUT,
    )
    .await;

    // An upgrade is scheduled for era 2, after the switch block in era 1 (height 2).
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
    }

    // Run until the nodes shut down for the upgrade.
    let timeout = Duration::from_secs(90);
    net.settle_on_exit(rng, ExitCode::Success, timeout).await;
}

#[tokio::test]
async fn nodes_should_have_enough_signatures_before_upgrade_with_equal_stake() {
    // Equal stake ensures that one node was able to learn about signatures created by the other, by
    // whatever means necessary (gossiping, broadcasting, fetching, etc.).
    testing::init_logging();

    let mut rng = crate::new_rng();

    let stakes = [U512::from(u128::MAX), U512::from(u128::MAX)];
    assert_network_shutdown_for_upgrade_with_stakes(&mut rng, &stakes).await;
}

#[tokio::test]
async fn nodes_should_have_enough_signatures_before_upgrade_with_one_dominant_stake() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    let stakes = [U512::from(u128::MAX), U512::from(u8::MAX)];
    assert_network_shutdown_for_upgrade_with_stakes(&mut rng, &stakes).await;
}

#[tokio::test]
async fn dont_upgrade_without_switch_block() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    eprintln!(
        "Running 'dont_upgrade_without_switch_block' test with rng={}",
        rng
    );

    const NETWORK_SIZE: usize = 2;
    const INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(20);

    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE, None);
    chain.chainspec_mut().core_config.minimum_era_height = 2;
    chain.chainspec_mut().core_config.era_duration = TimeDiff::from_millis(0);
    chain.chainspec_mut().core_config.minimum_block_time = "1second".parse().unwrap();

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    // Wait until initialization is finished, so upgrade watcher won't reject test requests.
    net.settle_on(
        &mut rng,
        move |nodes: &Nodes| {
            nodes
                .values()
                .all(|runner| !matches!(runner.main_reactor().state, ReactorState::Initialize))
        },
        INITIALIZATION_TIMEOUT,
    )
    .await;

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
                    executable_block, ..
                },
            ) = &event
            {
                if executable_block.era_report.is_some()
                    && executable_block.era_id == EraId::from(1)
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
        assert_eq!(EraId::from(1), header.era_id(), "era should be 1");
        assert!(header.is_switch_block(), "header should be switch block");
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

    deploy_alice_bob.sign(&alice_secret_key);
    deploy_alice_bob.sign(&bob_secret_key);

    deploy_alice_bob_charlie.sign(&alice_secret_key);
    deploy_alice_bob_charlie.sign(&bob_secret_key);
    deploy_alice_bob_charlie.sign(&charlie_secret_key);

    deploy_bob_alice.sign(&bob_secret_key);
    deploy_bob_alice.sign(&alice_secret_key);

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

    let deploy_hash = *DeployOrTransferHash::new(&deploy_alice_bob).deploy_hash();

    for runner in net.runners_mut() {
        let deploy = if runner.main_reactor().consensus().public_key() == &alice_public_key {
            // Alice will propose the deploy signed by Alice and Bob.
            deploy_alice_bob.clone()
        } else {
            // Bob will receive the deploy signed by Alice, Bob and Charlie.
            deploy_alice_bob_charlie.clone()
        };
        runner
            .process_injected_effects(|effect_builder| {
                effect_builder
                    .put_transaction_to_storage(Transaction::from(deploy.clone()))
                    .ignore()
            })
            .await;
        runner
            .process_injected_effects(|effect_builder| {
                effect_builder
                    .announce_new_transaction_accepted(
                        Arc::new(Transaction::from(deploy)),
                        Source::Client,
                    )
                    .ignore()
            })
            .await;
    }

    // Run until the deploy gets executed.
    let timeout = Duration::from_secs(120);
    net.settle_on(
        &mut rng,
        |nodes| {
            nodes.values().all(|runner| {
                runner
                    .main_reactor()
                    .storage()
                    .read_execution_result(&deploy_hash)
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
            .get_transaction_with_finalized_approvals_by_hash(&TransactionHash::from(deploy_hash))
            .map(|transaction_wfa| match transaction_wfa {
                TransactionWithFinalizedApprovals::Deploy {
                    deploy,
                    finalized_approvals,
                } => DeployWithFinalizedApprovals::new(deploy, finalized_approvals),
                _ => panic!("should receive deploy with finalized approvals"),
            });
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

// This test exercises a scenario in which a proposed block contains invalid accusations.
// Blocks containing no deploys or transfers used to be incorrectly marked as not needing
// validation even if they contained accusations, which opened up a security hole through which a
// malicious validator could accuse whomever they wanted of equivocating and have these
// accusations accepted by the other validators. This has been patched and the test asserts that
// such a scenario is no longer possible.
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
    let everyone_else: Vec<_> = stakes
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
            MainEvent::Consensus(consensus::Event::NewBlockPayload(NewBlockPayload {
                era_id,
                block_payload: _,
                block_context,
            })) => {
                info!("Accusing everyone else!");
                // We hook into the NewBlockPayload event to replace the block being proposed with
                // an empty one that accuses all the validators, except the malicious validator.
                Either::Right(MainEvent::Consensus(consensus::Event::NewBlockPayload(
                    NewBlockPayload {
                        era_id,
                        block_payload: Arc::new(BlockPayload::new(
                            vec![],
                            vec![],
                            everyone_else.clone(),
                            Default::default(),
                            false,
                        )),
                        block_context,
                    },
                )))
            }
            event => Either::Right(event),
        });

    let timeout = Duration::from_secs(300);
    info!("Waiting for the first era after genesis to end.");
    net.settle_on(&mut rng, is_in_era(EraId::new(2)), timeout)
        .await;
    let switch_blocks = SwitchBlocks::collect(net.nodes(), 2);

    // Nobody actually double-signed. The accusations should have had no effect.
    assert_eq!(
        switch_blocks.equivocators(0),
        [],
        "expected no equivocators"
    );
    // If the malicious validator was the first proposer, all their Highway units might be invalid,
    // because they all refer to the invalid proposal, so they might get flagged as inactive. No
    // other validators should be considered inactive.
    match switch_blocks.inactive_validators(0) {
        [] => {}
        [inactive_validator] if malicious_validator == *inactive_validator => {}
        inactive => panic!("unexpected inactive validators: {:?}", inactive),
    }
}

fn simple_test_chain(
    named: Vec<(Arc<SecretKey>, PublicKey, U512)>,
    total_node_count: usize,
    nameless_stake_override: Option<U512>,
    rng: &mut TestRng,
) -> TestChain {
    // getting a basic network set up
    if total_node_count < named.len() {
        panic!("named validator count exceeds total node count");
    }

    let diff = total_node_count - named.len();
    // Leave open slots for named validators.
    let mut keys: Vec<Arc<SecretKey>> = (1..=diff)
        .map(|_| Arc::new(SecretKey::random(rng)))
        .collect();
    // Nameless validators stake; default == 100 token
    let nameless_stake = nameless_stake_override.unwrap_or(U512::from(10_000_000_000u64));
    let mut stakes: BTreeMap<PublicKey, U512> = keys
        .iter()
        .map(|secret_key| (PublicKey::from(&*secret_key.clone()), nameless_stake))
        .collect();

    for (secret_key, public_key, stake) in named {
        keys.push(secret_key.clone());
        stakes.insert(public_key, stake);
    }

    let validator_count = keys.len();
    let mut chain = TestChain::new_with_keys(rng, keys, stakes.clone());
    chain.chainspec_mut().core_config.minimum_era_height = 5;
    chain.chainspec_mut().highway_config.maximum_round_length =
        chain.chainspec.core_config.minimum_block_time * 2;
    chain.chainspec_mut().core_config.validator_slots = validator_count as u32;
    chain.chainspec_mut().core_config.locked_funds_period = TimeDiff::default();
    chain.chainspec_mut().core_config.vesting_schedule_period = TimeDiff::default();
    chain.chainspec_mut().core_config.minimum_delegation_amount = 0;
    chain.chainspec_mut().core_config.unbonding_delay = 1;
    chain
}

fn get_highest_block_or_fail(
    runner: &Runner<ConditionCheckReactor<FilterReactor<MainReactor>>>,
) -> Block {
    runner
        .main_reactor()
        .storage
        .read_highest_block()
        .expect("should not have have storage error")
        .expect("should have block")
}

fn get_system_contract_hash_or_fail(
    net: &TestingNetwork<FilterReactor<MainReactor>>,
    public_key: &PublicKey,
    system_contract_name: String,
) -> ContractHash {
    let (_, runner) = net
        .nodes()
        .iter()
        .find(|(_, x)| x.main_reactor().consensus.public_key() == public_key)
        .expect("should have alice runner");

    let highest_block = get_highest_block_or_fail(runner);

    let state_hash = highest_block.state_root_hash();
    // we need the native auction addr so we can directly call it w/o wasm
    // we can get it out of the system contract registry which is just a
    // value in global state under a stable key.
    let maybe_registry = runner
        .main_reactor()
        .contract_runtime
        .engine_state()
        .get_state()
        .checkout(*state_hash)
        .expect("should checkout")
        .expect("should have view")
        .read(&Key::SystemContractRegistry)
        .expect("should not have gs storage error")
        .expect("should have stored value");

    let system_contract_registry: SystemContractRegistry = match maybe_registry {
        StoredValue::CLValue(cl_value) => CLValue::into_t(cl_value).unwrap(),
        _ => {
            panic!("expected CLValue")
        }
    };

    *system_contract_registry.get(&system_contract_name).unwrap()
}

fn check_bid_existence_at_tip(
    net: &TestingNetwork<FilterReactor<MainReactor>>,
    validator_public_key: &PublicKey,
    delegator_public_key: Option<&PublicKey>,
    should_exist: bool,
) {
    let (_, runner) = net
        .nodes()
        .iter()
        .find(|(_, x)| x.main_reactor().consensus.public_key() == validator_public_key)
        .expect("should have runner");

    let highest_block = get_highest_block_or_fail(runner);

    let state_hash = highest_block.state_root_hash();

    let bids_result = runner
        .main_reactor()
        .contract_runtime
        .auction_state(*state_hash)
        .expect("should have bids result");

    if let GetBidsResult::Success { bids } = bids_result {
        match bids.iter().find(|x| {
            &x.validator_public_key() == validator_public_key
                && x.delegator_public_key().as_ref() == delegator_public_key
        }) {
            None => {
                if should_exist {
                    panic!("should have bid era: {}", highest_block.era_id());
                }
            }
            Some(bid) => {
                if !should_exist {
                    info!("unexpected bid record existence: {:?}", bid);
                    panic!("expected alice to not have bid");
                }
            }
        }
    } else {
        panic!("network should have bids");
    }
}

async fn settle_era(
    net: &mut TestingNetwork<FilterReactor<MainReactor>>,
    rng: &mut TestRng,
    era_id: &mut EraId,
    era_settle_duration: Duration,
) {
    info!(?era_id, "settling network on era");
    net.settle_on(rng, has_completed_era(*era_id), era_settle_duration)
        .await;
    era_id.increment();
}

async fn settle_deploy(net: &mut TestingNetwork<FilterReactor<MainReactor>>, deploy: Deploy) {
    // saturate the network with the deploy via just making them all store and accept it
    // they're all validators so one of them should propose it
    for runner in net.runners_mut() {
        let txn = Transaction::from(deploy.clone());
        runner
            .process_injected_effects(|effect_builder| {
                effect_builder
                    .put_transaction_to_storage(txn.clone())
                    .ignore()
            })
            .await;
        runner
            .process_injected_effects(|effect_builder| {
                effect_builder
                    .announce_new_transaction_accepted(Arc::new(txn), Source::Client)
                    .ignore()
            })
            .await;
    }
}

async fn settle_execution(
    net: &mut TestingNetwork<FilterReactor<MainReactor>>,
    rng: &mut TestRng,
    key: Key,
    deploy_hash: DeployHash,
    timeout: Duration,
    success_condition: fn(key: Key, effects: Effects) -> bool,
    failure_condition: fn(key: Key, error_message: String, cost: U512) -> bool,
) {
    net.settle_on(
        rng,
        |nodes| {
            nodes.values().all(|runner| {
                match runner
                    .main_reactor()
                    .storage()
                    .read_execution_result(&deploy_hash)
                {
                    Some(ExecutionResult::V2(execution_result)) => match execution_result {
                        ExecutionResultV2::Failure {
                            error_message,
                            cost,
                            ..
                        } => failure_condition(key, error_message, cost),
                        ExecutionResultV2::Success { effects, .. } => {
                            success_condition(key, effects)
                        }
                    },
                    Some(_) => panic!("unexpected execution result variant"),
                    None => false, // keep waiting
                }
            })
        },
        timeout,
    )
    .await;
}

#[tokio::test]
async fn run_withdraw_bid_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();
    let mut era_id = EraId::new(0);
    let era_timeout = Duration::from_secs(90);
    let execution_timeout = Duration::from_secs(120);
    let deploy_ttl = TimeDiff::from_seconds(60);
    let alice_secret_key = Arc::new(SecretKey::random(&mut rng));
    let alice_stake = U512::from(200_000_000_000u64);
    let alice_public_key = PublicKey::from(&*alice_secret_key);

    let (mut net, chain_name) = {
        let named_validators = vec![(
            alice_secret_key.clone(),
            alice_public_key.clone(),
            alice_stake,
        )];

        let mut chain = simple_test_chain(named_validators, 5, None, &mut rng);

        let chain_name = chain.chainspec.network_config.name.clone();

        (
            chain
                .create_initialized_network(&mut rng)
                .await
                .expect("network initialization failed"),
            chain_name,
        )
    };

    // genesis
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    // making sure our post genesis assumption that alice has a bid is correct
    check_bid_existence_at_tip(&net, &alice_public_key, None, true);

    /* Now that the preamble is behind us, lets get some work done */

    // crank the network forward
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    let (deploy_hash, deploy) = {
        let auction_hash = get_system_contract_hash_or_fail(
            &net,
            &alice_public_key,
            casper_types::system::AUCTION.to_string(),
        );

        // create & sign deploy to withdraw alice's full stake
        let mut withdraw_bid = Deploy::withdraw_bid(
            chain_name,
            auction_hash,
            alice_public_key.clone(),
            alice_stake,
            Timestamp::now(),
            deploy_ttl,
        );
        // sign the deploy (single sig in this scenario)
        withdraw_bid.sign(&alice_secret_key);
        // we'll need the deploy_hash later to check results
        let deploy_hash = *DeployOrTransferHash::new(&withdraw_bid).deploy_hash();
        (deploy_hash, withdraw_bid)
    };
    settle_deploy(&mut net, deploy).await;

    let bid_key = Key::BidAddr(BidAddr::from(alice_public_key.clone()));
    settle_execution(
        &mut net,
        &mut rng,
        bid_key,
        deploy_hash,
        execution_timeout,
        |key, effects| {
            let _ = effects
                .transforms()
                .iter()
                .find(|x| match x.kind() {
                    TransformKind::Prune(prune_key) => prune_key == &key,
                    _ => false,
                })
                .expect("should have a prune record for bid");
            true
        },
        |key, error_message, cost| {
            panic!("{:?} deploy failed: {} cost: {}", key, error_message, cost);
        },
    )
    .await;

    // crank the network forward
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    check_bid_existence_at_tip(&net, &alice_public_key, None, false);
}

#[tokio::test]
async fn run_undelegate_bid_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();
    let mut era_id = EraId::new(0);
    let era_timeout = Duration::from_secs(90);
    let execution_timeout = Duration::from_secs(120);
    let deploy_ttl = TimeDiff::from_seconds(60);
    let alice = {
        let secret_key = Arc::new(SecretKey::random(&mut rng));
        let public_key = PublicKey::from(&*secret_key);
        let stake = U512::from(200_000_000_000u64);
        (secret_key, public_key, stake)
    };
    let bob = {
        let secret_key = Arc::new(SecretKey::random(&mut rng));
        let public_key = PublicKey::from(&*secret_key);
        let stake = U512::from(300_000_000_000u64);
        (secret_key, public_key, stake)
    };

    let (mut net, chain_name) = {
        let named_validators = vec![alice.clone(), bob.clone()];

        let mut chain = simple_test_chain(named_validators, 5, None, &mut rng);

        let chain_name = chain.chainspec.network_config.name.clone();

        (
            chain
                .create_initialized_network(&mut rng)
                .await
                .expect("network initialization failed"),
            chain_name,
        )
    };

    // genesis
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    // making sure our post genesis assumptions are correct
    check_bid_existence_at_tip(&net, &alice.1, None, true);
    check_bid_existence_at_tip(&net, &bob.1, None, true);
    // alice should not have a delegation bid record for bob (yet)
    check_bid_existence_at_tip(&net, &bob.1, Some(&alice.1), false);

    /* Now that the preamble is behind us, lets get some work done */

    // crank the network forward
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    let auction_hash =
        get_system_contract_hash_or_fail(&net, &alice.1, casper_types::system::AUCTION.to_string());

    // have alice delegate to bob
    // note, in the real world validators usually don't also delegate to other validators
    // but in this test fixture the only accounts in the system are those created for
    // genesis validators.
    let alice_delegation_amount = U512::from(50_000_000_000u64);
    let (deploy_hash, deploy) = {
        // create & sign deploy to delegate from alice to bob
        let mut delegate = Deploy::delegate(
            chain_name.clone(),
            auction_hash,
            bob.1.clone(),
            alice.1.clone(),
            alice_delegation_amount,
            Timestamp::now(),
            deploy_ttl,
        );
        // sign the deploy (single sig in this scenario)
        delegate.sign(&alice.0);
        // we'll need the deploy_hash later to check results
        let deploy_hash = *DeployOrTransferHash::new(&delegate).deploy_hash();
        (deploy_hash, delegate)
    };
    settle_deploy(&mut net, deploy).await;

    let bid_key = Key::BidAddr(BidAddr::new_from_public_keys(
        &bob.1.clone(),
        Some(&alice.1.clone()),
    ));

    settle_execution(
        &mut net,
        &mut rng,
        bid_key,
        deploy_hash,
        execution_timeout,
        |key, effects| {
            let _ = effects
                .transforms()
                .iter()
                .find(|x| match x.kind() {
                    TransformKind::Write(StoredValue::BidKind(bid_kind)) => {
                        Key::from(bid_kind.bid_addr()) == key
                    }
                    _ => false,
                })
                .expect("should have a write record for delegate bid");
            true
        },
        |key, error_message, cost| {
            panic!("{:?} deploy failed: {} cost: {}", key, error_message, cost);
        },
    )
    .await;

    // alice should now have a delegation bid record for bob
    check_bid_existence_at_tip(&net, &bob.1, Some(&alice.1), true);

    let (deploy_hash, deploy) = {
        // create & sign deploy to undelegate from alice to bob
        let mut undelegate = Deploy::undelegate(
            chain_name,
            auction_hash,
            bob.1.clone(),
            alice.1.clone(),
            alice_delegation_amount,
            Timestamp::now(),
            deploy_ttl,
        );
        // sign the deploy (single sig in this scenario)
        undelegate.sign(&alice.0);
        // we'll need the deploy_hash later to check results
        let deploy_hash = *DeployOrTransferHash::new(&undelegate).deploy_hash();
        (deploy_hash, undelegate)
    };
    settle_deploy(&mut net, deploy).await;

    settle_execution(
        &mut net,
        &mut rng,
        bid_key,
        deploy_hash,
        execution_timeout,
        |key, effects| {
            let _ = effects
                .transforms()
                .iter()
                .find(|x| match x.kind() {
                    TransformKind::Prune(prune_key) => prune_key == &key,
                    _ => false,
                })
                .expect("should have a prune record for undelegated bid");
            true
        },
        |key, error_message, cost| {
            panic!("{:?} deploy failed: {} cost: {}", key, error_message, cost);
        },
    )
    .await;

    // crank the network forward
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    // making sure the validator records are still present but the undelegated bid is gone
    check_bid_existence_at_tip(&net, &alice.1, None, true);
    check_bid_existence_at_tip(&net, &bob.1, None, true);
    check_bid_existence_at_tip(&net, &bob.1, Some(&alice.1), false);
}

#[tokio::test]
async fn run_redelegate_bid_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();
    let mut era_id = EraId::new(0);
    let era_timeout = Duration::from_secs(90);
    let execution_timeout = Duration::from_secs(120);
    let deploy_ttl = TimeDiff::from_seconds(60);
    let alice = {
        let secret_key = Arc::new(SecretKey::random(&mut rng));
        let public_key = PublicKey::from(&*secret_key);
        let stake = U512::from(200_000_000_000u64);
        (secret_key, public_key, stake)
    };
    let bob = {
        let secret_key = Arc::new(SecretKey::random(&mut rng));
        let public_key = PublicKey::from(&*secret_key);
        let stake = U512::from(300_000_000_000u64);
        (secret_key, public_key, stake)
    };
    let charlie = {
        let secret_key = Arc::new(SecretKey::random(&mut rng));
        let public_key = PublicKey::from(&*secret_key);
        let stake = U512::from(300_000_000_000u64);
        (secret_key, public_key, stake)
    };

    let (mut net, chain_name) = {
        let named_validators = vec![alice.clone(), bob.clone(), charlie.clone()];

        let mut chain = simple_test_chain(named_validators, 5, None, &mut rng);

        let chain_name = chain.chainspec.network_config.name.clone();

        (
            chain
                .create_initialized_network(&mut rng)
                .await
                .expect("network initialization failed"),
            chain_name,
        )
    };

    // genesis
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    // making sure our post genesis assumptions are correct
    check_bid_existence_at_tip(&net, &alice.1, None, true);
    check_bid_existence_at_tip(&net, &bob.1, None, true);
    check_bid_existence_at_tip(&net, &charlie.1, None, true);
    // alice should not have a delegation bid record for bob or charlie (yet)
    check_bid_existence_at_tip(&net, &bob.1, Some(&alice.1), false);
    check_bid_existence_at_tip(&net, &charlie.1, Some(&alice.1), false);

    /* Now that the preamble is behind us, lets get some work done */

    // crank the network forward
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    let auction_hash =
        get_system_contract_hash_or_fail(&net, &alice.1, casper_types::system::AUCTION.to_string());

    // have alice delegate to bob
    let alice_delegation_amount = U512::from(50_000_000_000u64);
    let (deploy_hash, deploy) = {
        // create & sign deploy to delegate from alice to bob
        let mut delegate = Deploy::delegate(
            chain_name.clone(),
            auction_hash,
            bob.1.clone(),
            alice.1.clone(),
            alice_delegation_amount,
            Timestamp::now(),
            deploy_ttl,
        );
        // sign the deploy (single sig in this scenario)
        delegate.sign(&alice.0);
        // we'll need the deploy_hash later to check results
        let deploy_hash = *DeployOrTransferHash::new(&delegate).deploy_hash();
        (deploy_hash, delegate)
    };
    settle_deploy(&mut net, deploy).await;

    let bid_key = Key::BidAddr(BidAddr::new_from_public_keys(
        &bob.1.clone(),
        Some(&alice.1.clone()),
    ));

    settle_execution(
        &mut net,
        &mut rng,
        bid_key,
        deploy_hash,
        execution_timeout,
        |key, effects| {
            let _ = effects
                .transforms()
                .iter()
                .find(|x| match x.kind() {
                    TransformKind::Write(StoredValue::BidKind(bid_kind)) => {
                        Key::from(bid_kind.bid_addr()) == key
                    }
                    _ => false,
                })
                .expect("should have a write record for delegate bid");
            true
        },
        |key, error_message, cost| {
            panic!("{:?} deploy failed: {} cost: {}", key, error_message, cost);
        },
    )
    .await;

    // alice should now have a delegation bid record for bob
    check_bid_existence_at_tip(&net, &bob.1, Some(&alice.1), true);

    let (deploy_hash, deploy) = {
        // create & sign deploy to undelegate alice from bob and delegate to charlie
        let mut redelegate = Deploy::redelegate(
            chain_name,
            auction_hash,
            bob.1.clone(),
            alice.1.clone(),
            charlie.1.clone(),
            alice_delegation_amount,
            Timestamp::now(),
            deploy_ttl,
        );
        // sign the deploy (single sig in this scenario)
        redelegate.sign(&alice.0);
        // we'll need the deploy_hash later to check results
        let deploy_hash = *DeployOrTransferHash::new(&redelegate).deploy_hash();
        (deploy_hash, redelegate)
    };
    settle_deploy(&mut net, deploy).await;

    settle_execution(
        &mut net,
        &mut rng,
        bid_key,
        deploy_hash,
        execution_timeout,
        |key, effects| {
            let _ = effects
                .transforms()
                .iter()
                .find(|x| match x.kind() {
                    TransformKind::Prune(prune_key) => prune_key == &key,
                    _ => false,
                })
                .expect("should have a prune record for undelegated bid");
            true
        },
        |key, error_message, cost| {
            panic!("{:?} deploy failed: {} cost: {}", key, error_message, cost);
        },
    )
    .await;

    // original delegation bid should be removed
    check_bid_existence_at_tip(&net, &bob.1, Some(&alice.1), false);
    // redelegate doesn't occur until after unbonding delay elapses
    check_bid_existence_at_tip(&net, &charlie.1, Some(&alice.1), false);

    // crank the network forward to run out the unbonding delay
    // first, close out the era the redelegate was processed in
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;
    // the undelegate is in the unbonding queue
    check_bid_existence_at_tip(&net, &charlie.1, Some(&alice.1), false);
    // unbonding delay is 1 on this test network, so step 1 more era
    settle_era(&mut net, &mut rng, &mut era_id, era_timeout).await;

    // making sure the validator records are still present
    check_bid_existence_at_tip(&net, &alice.1, None, true);
    check_bid_existence_at_tip(&net, &bob.1, None, true);
    // redelegated bid exists
    check_bid_existence_at_tip(&net, &charlie.1, Some(&alice.1), true);
}
