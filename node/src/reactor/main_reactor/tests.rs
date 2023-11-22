mod builder;

use std::{collections::BTreeMap, convert::TryFrom, iter, sync::Arc, time::Duration};
use std::cmp::max;
use std::collections::BTreeSet;

use either::Either;
use num::Zero;
use num_rational::Ratio;
use rand::Rng;
use tempfile::TempDir;
use tokio::time;
use tracing::{error, info};

use builder::TestChainBuilder;
use casper_execution_engine::engine_state::{Error, GetBidsRequest, GetBidsResult, QueryRequest, QueryResult, SystemContractRegistry};
use casper_execution_engine::engine_state::QueryResult::{CircularReference, DepthLimit, Success, ValueNotFound};
use casper_storage::global_state::state::{StateProvider, StateReader};
use casper_types::{execution::{Effects, ExecutionResult, ExecutionResultV2, TransformKind}, system::auction::{BidAddr, BidKind, BidsExt, DelegationRate}, testing::TestRng, AccountConfig, AccountsConfig, ActivationPoint, AddressableEntityHash, Block, BlockHash, BlockHeader, BlockV2, CLValue, Chainspec, ChainspecRawBytes, Deploy, DeployHash, EraId, Key, Motes, ProtocolVersion, PublicKey, SecretKey, StoredValue, TimeDiff, Timestamp, Transaction, TransactionHash, ValidatorConfig, U512, ConsensusProtocolName, Rewards};
use casper_types::Key::AddressableEntity;
use casper_types::package::PackageKindTag;
use casper_types::system::mint;

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
use crate::failpoints::FailpointActivation;
use crate::reactor::Reactor;

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
        Self::new_with_keys(keys, stakes)
    }

    /// Instantiates a new test chain configuration.
    ///
    /// Takes a vector of bonded keys with specified bond amounts.
    fn new_with_keys(
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
                AccountConfig::new(public_key, Motes::new(U512::zero()), Some(validator_config))
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

fn switch_block_for_era(
    runner: &Runner<ConditionCheckReactor<FilterReactor<MainReactor>>>,
    era_id: EraId,
) -> Option<BlockV2> {
    let storage = runner.main_reactor().storage();
    storage
        .read_switch_block_by_era_id(era_id)
        .unwrap_or_default()
        .and_then(|block| BlockV2::try_from(block).ok())
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
    let mut chain = TestChain::new_with_keys(keys, stakes.clone());
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
    let mut chain = TestChain::new_with_keys(keys, stakes.clone());
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
    let mut chain = TestChain::new_with_keys(keys, stakes.clone());
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
    let mut chain = TestChain::new_with_keys(keys, stakes.clone());
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
) -> AddressableEntityHash {
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

#[tokio::test]
async fn rewards_are_calculated() {
    testing::init_logging();

    let rng = &mut crate::new_rng();

    // Instantiate a new chain with a fixed size.
    const NETWORK_SIZE: usize = 5;
    let (_, mut network) = TestChainBuilder::new_with_size(NETWORK_SIZE)
        .chainspec(|chainspec| {
            chainspec.core_config.minimum_era_height = 3;
        })
        .build(rng)
        .await;

    let era = EraId::new(2);

    network
        .settle_on(rng, has_completed_era(era), Duration::from_secs(150))
        .await;

    let switch_block =
        switch_block_for_era(network.first_node(), era).expect("a switch block for era 2");

    for reward in switch_block.era_end().unwrap().rewards().values() {
        assert_ne!(reward, &U512::zero());
    }
}

// Fundamental network parameters that are not critical for assessing reward calculation correctness
const VALIDATOR_SLOTS: u32 = 10;
const NETWORK_SIZE: u64 = 10;
const STAKE: u64 = 1000000000;
const PRIME_STAKES: &'static [u64; 5] = &[106907u64, 106921u64, 106937u64, 106949u64, 106957u64];
const ERA_COUNT: u64 = 3;
const ERA_DURATION: u64 = 30000; //milliseconds
const MIN_HEIGHT: u64 = 10;
const BLOCK_TIME: u64 = 3000; //milliseconds
const TIME_OUT: u64 = 3000; //seconds
const SEIGNIORAGE: (u64, u64) = (1u64, 100u64);
const REPRESENTATIVE_NODE_INDEX: usize = 0;
// Parameters we generally want to vary
const CONSENSUS_ZUG: ConsensusProtocolName = ConsensusProtocolName::Zug;
const CONSENSUS_HIGHWAY: ConsensusProtocolName = ConsensusProtocolName::Highway;
const FINDERS_FEE_ZERO: (u64, u64) = (0u64, 1u64);
const FINDERS_FEE_HALF: (u64, u64) = (1u64, 2u64);
const FINDERS_FEE_ONE: (u64, u64) = (1u64, 1u64);
const FINALITY_SIG_PROP_ZERO: (u64, u64) = (0u64, 1u64);
const FINALITY_SIG_PROP_HALF: (u64, u64) = (1u64, 2u64);
const FINALITY_SIG_PROP_ONE: (u64, u64) = (1u64, 1u64);
const FILTERED_NODES_INDICES: &'static [usize] = &[3, 4];
const FINALITY_SIG_LOOKBACK: u64 = 3;

async fn run_rewards_network_scenario(
    mut rng: NodeRng,
    consensus: ConsensusProtocolName,
    initial_stakes: &[u64],
    era_count: u64,
    era_duration: u64, //milliseconds
    min_height: u64,
    block_time: u64, //milliseconds
    time_out: u64, //seconds
    seigniorage: (u64, u64),
    finders_fee: (u64, u64),
    finality_sig_prop: (u64, u64),
    finality_lookback: u64,
    representative_node_index: usize,
    filtered_nodes_indices: &[usize]) {

    testing::init_logging();

    // Create random keypairs to populate our network
    let keys: Vec<Arc<SecretKey>> = (1..&initial_stakes.len() + 1)
        .map(|_| Arc::new(SecretKey::random(&mut rng)))
        .collect();
    let stakes: BTreeMap<PublicKey, U512> = keys
        .iter()
        .enumerate()
        .map(|(i, secret_key)| (PublicKey::from(&*secret_key.clone()), U512::from(*&initial_stakes[i])))
        .collect();

    // Instantiate the chain
    let mut chain = TestChain::new_with_keys(keys, stakes.clone());

    chain.chainspec_mut().core_config.validator_slots = *&stakes.len() as u32;
    chain.chainspec_mut().core_config.era_duration = TimeDiff::from_millis(era_duration);
    chain.chainspec_mut().core_config.minimum_era_height = min_height;
    chain.chainspec_mut().core_config.minimum_block_time = TimeDiff::from_millis(block_time);
    chain.chainspec_mut().core_config.round_seigniorage_rate = Ratio::from(seigniorage);
    chain.chainspec_mut().core_config.consensus_protocol = consensus;
    chain.chainspec_mut().core_config.finders_fee = Ratio::from(finders_fee);
    chain.chainspec_mut().core_config.finality_signature_proportion = Ratio::from(finality_sig_prop);
    chain.chainspec_mut().core_config.signature_rewards_max_delay = finality_lookback;

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    for i in filtered_nodes_indices {
        let filtered_node = net.runners_mut().nth(*i).unwrap();
        filtered_node
            .reactor_mut()
            .inner_mut()
            .activate_failpoint(&FailpointActivation::new("finality_signature_creation"));
    }

    // Run the network for a specified number of eras
    // TODO: Consider replacing era duration estimate with actual chainspec value
    let timeout = Duration::from_secs(time_out);
    net.settle_on(
        &mut rng,
        has_completed_era(EraId::new(era_count - 1)),
        timeout)
        .await;

    // DATA COLLECTION
    // Get the switch blocks and bid structs first
    let switch_blocks = SwitchBlocks::collect(net.nodes(), era_count);

    // Representative node
    // (this test should normally run a network at nominal performance with identical nodes)
    let representative_node = net.nodes().values().nth(representative_node_index).unwrap();
    let representative_storage = &representative_node.main_reactor().storage;
    let representative_runtime = &representative_node.main_reactor().contract_runtime;

    // Recover highest completed block height
    let highest_completed_height = representative_storage
        .highest_complete_block_height()
        .expect("missing highest completed block");

    // Get all the blocks
    let blocks: Vec<Block> =
        (0..highest_completed_height + 1).map(
            |i| representative_storage.read_block_by_height(i).expect("block not found").unwrap()
        ).collect();

    // Recover history of total supply
    let mint_hash: AddressableEntityHash = {
        let any_state_hash = *switch_blocks.headers[0].state_root_hash();
        representative_runtime
            .engine_state()
            .get_system_mint_hash(any_state_hash)
            .expect("mint contract hash not found")
    };

    // Get total supply history
    let total_supply: Vec<U512> = (0..highest_completed_height + 1)
        .map(|height: u64| {
            let state_hash = *representative_storage
                .read_block_header_by_height(height, true)
                .expect("failure to read block header")
                .unwrap()
                .state_root_hash();

            let request = QueryRequest::new(
                state_hash.clone(),
                AddressableEntity(PackageKindTag::System, mint_hash.value()),
                vec![mint::TOTAL_SUPPLY_KEY.to_owned()],
            );

            representative_runtime
                .engine_state()
                .run_query(request)
                .and_then(move |query_result| match query_result {
                    Success { value, proofs: _ } => value
                        .as_cl_value()
                        .ok_or_else(|| Error::Mint("Value not a CLValue".to_owned()))?
                        .clone()
                        .into_t::<U512>()
                        .map_err(|e| Error::Mint(format!("CLValue not a U512: {e}"))),
                    ValueNotFound(s) => Err(Error::Mint(format!("ValueNotFound({s})"))),
                    CircularReference(s) => Err(Error::Mint(format!("CircularReference({s})"))),
                    DepthLimit { depth } => Err(Error::Mint(format!("DepthLimit({depth})"))),
                    QueryResult::RootNotFound => Err(Error::RootNotFound(state_hash)),
                })
                .expect("failure to recover total supply")
        })
        .collect();

    // Tiny helper function
    #[inline]
    fn add_to_rewards(recipient: PublicKey, reward: Ratio<u64>, rewards: &mut BTreeMap<PublicKey, Ratio<u64>>, era: usize, total_supply: &mut BTreeMap<usize, Ratio<u64>>) {
        match (rewards.get_mut(&recipient.clone()), total_supply.get_mut(&era)) {
            (Some(value), Some(supply)) => {
                *value += reward;
                *supply += reward;
            }
            (None, Some(supply)) => {
                rewards.insert(recipient.clone(), reward);
                *supply += reward;
            }
            (Some(_), None) => panic!("rewards present without corresponding supply increase"),
            (None, None) => {
                total_supply.insert(era, reward);
                rewards.insert(recipient.clone(), reward);
            }
        }
    }

    let mut recomputed_total_supply = BTreeMap::<usize, Ratio<u64>>::new();
    recomputed_total_supply.insert(0, Ratio::from(total_supply[0].as_u64()));
    let recomputed_rewards = switch_blocks.headers
        .iter()
        .enumerate()
        .map(|(i, switch_block)|
            if switch_block.is_genesis() || switch_block.height() > highest_completed_height {
                return (i, BTreeMap::<PublicKey, Ratio<u64>>::new())
            } else {
                let mut recomputed_era_rewards = BTreeMap::<PublicKey, Ratio<u64>>::new();
                if !(switch_block.is_genesis()) {
                    let supply_carryover = recomputed_total_supply.get(&(&i - &1usize)).expect("expected prior recomputed supply value").clone();
                    recomputed_total_supply.insert(i, supply_carryover);
                }

                // It's not a genesis block, so we know there's something with a lower era id
                let previous_switch_block_height = switch_blocks.headers[i - 1].height();
                let current_era_slated_weights =
                    match switch_blocks.headers[i - 1].clone_era_end() {
                        Some(era_report) => era_report.next_era_validator_weights().clone(),
                        _ => panic!("unexpectedly absent era report"),
                    };
                let total_current_era_weights = current_era_slated_weights
                    .iter()
                    .fold(0u64, move |acc, s| acc + s.1.as_u64());
                let (previous_era_slated_weights, total_previous_era_weights) =
                    if switch_blocks.headers[i - 1].is_genesis() {
                        (None, None)
                    } else {
                        match switch_blocks.headers[i - 2].clone_era_end() {
                            Some(era_report) => {
                                let next_weights = era_report.next_era_validator_weights().clone();
                                let total_next_weights = next_weights
                                    .iter()
                                    .fold(0u64, move |acc, s| acc + s.1.as_u64());
                                (Some(next_weights), Some(total_next_weights))},
                            _ => panic!("unexpectedly absent era report"),
                        }
                    };

                // TODO: Investigate whether the rewards pay out for the signatures _in the switch block itself_
                let rewarded_range = previous_switch_block_height as usize + 1..switch_block.height() as usize + 1;
                let rewarded_blocks = &blocks[rewarded_range];
                let block_reward =
                 (Ratio::new(1, 1) - chain.chainspec.core_config.finality_signature_proportion)
                    * Ratio::from(recomputed_total_supply[&(i - 1)]) * chain.chainspec.core_config.round_seigniorage_rate;
                let signatures_reward = chain.chainspec.core_config.finality_signature_proportion
                    * Ratio::from(recomputed_total_supply[&(i - 1)]) * chain.chainspec.core_config.round_seigniorage_rate;
                let previous_signatures_reward =
                    if switch_blocks.headers[i - 1].is_genesis() {
                        None
                    } else {
                        Some(chain.chainspec.core_config.finality_signature_proportion
                            * Ratio::from(recomputed_total_supply[&(i - 2)]) * chain.chainspec.core_config.round_seigniorage_rate)
                    };

                rewarded_blocks
                    .iter()
                    .for_each(|block: &Block| {
                        // Block production rewards
                        let proposer = block.proposer().clone();
                        add_to_rewards(proposer.clone(), block_reward.trunc(), &mut recomputed_era_rewards, i, &mut recomputed_total_supply);

                        // Recover relevant finality signatures
                        // TODO: Deal with the implicit assumption that lookback only look backs one previous era
                        block.rewarded_signatures()
                            .iter()
                            .enumerate()
                            .for_each(|(offset, signatures_packed)| {
                                if block.height() as usize - offset - 1 <= previous_switch_block_height as usize && !switch_blocks.headers[i - 1].is_genesis() {
                                    let rewarded_contributors = signatures_packed.to_validator_set(previous_era_slated_weights.as_ref().expect("expected previous era weights").keys().cloned().collect::<BTreeSet<PublicKey>>());
                                    rewarded_contributors
                                        .iter()
                                        .for_each(|contributor| {
                                            let contributor_proportion = Ratio::new(previous_era_slated_weights.as_ref().expect("expected previous era weights")
                                                .get(contributor)
                                                .expect("expected current era validator").as_u64(),
                                                total_previous_era_weights.expect("expected total previous era weight"));
                                            add_to_rewards(proposer.clone(), (chain.chainspec.core_config.finders_fee * contributor_proportion * previous_signatures_reward.unwrap()).trunc(), &mut recomputed_era_rewards, i, &mut recomputed_total_supply);
                                            add_to_rewards(contributor.clone(), ((Ratio::new(1, 1) - chain.chainspec.core_config.finders_fee) * contributor_proportion * previous_signatures_reward.unwrap()).trunc(), &mut recomputed_era_rewards, i, &mut recomputed_total_supply)
                                        });
                                } else {
                                    let rewarded_contributors = signatures_packed.to_validator_set(current_era_slated_weights.keys().map(|key| key.clone()).collect::<BTreeSet<PublicKey>>());
                                    rewarded_contributors
                                        .iter()
                                        .for_each(|contributor| {
                                            let contributor_proportion = Ratio::new(current_era_slated_weights
                                                .get(contributor)
                                                .expect("expected current era validator").as_u64(),
                                                total_current_era_weights);
                                            add_to_rewards(proposer.clone(), (chain.chainspec.core_config.finders_fee * contributor_proportion * signatures_reward).trunc(), &mut recomputed_era_rewards, i, &mut recomputed_total_supply);
                                            add_to_rewards(contributor.clone(), ((Ratio::new(1, 1) - chain.chainspec.core_config.finders_fee) * contributor_proportion * signatures_reward).trunc(), &mut recomputed_era_rewards, i, &mut recomputed_total_supply);
                                        });
                                }
                            });
                    });
                return (i, recomputed_era_rewards) }
        )
        .collect::<BTreeMap<usize,BTreeMap<PublicKey, Ratio<u64>>>>();

    // Recalculated total supply is equal to observed total supply
    switch_blocks.headers
        .iter()
        .for_each(|header|
            if header.height() <= highest_completed_height {
                assert_eq!(Ratio::<u64>::from(total_supply[header.height() as usize].as_u64()), *(recomputed_total_supply.get(&(header.era_id().value() as usize)).expect("expected recalculated supply")), "total supply does not match at height {}", header.height())
            } else {}
        );

    // Recalculated rewards are equal to observed rewards; total supply increase is equal to total rewards;
    recomputed_rewards
        .iter()
        .for_each(|(era, rewards)| {
            if era > &0 && switch_blocks.headers[*era].height() <= highest_completed_height {
                let observed_total_rewards = match switch_blocks.headers[*era].clone_era_end().expect("expected EraEnd").rewards() {
                    Rewards::V1(v1_rewards) => v1_rewards
                        .iter()
                        .fold(Ratio::from(0u64), |acc, reward| Ratio::from(*(reward.1)) + acc),
                    Rewards::V2(v2_rewards) => v2_rewards
                        .iter()
                        .fold(Ratio::from(0u64), |acc, reward| Ratio::<u64>::from(reward.1.as_u64()) + acc),
                };
                let recomputed_total_rewards = rewards.iter().fold(Ratio::from(0u64), |acc, x| x.1 + acc);
                assert_eq!(Ratio::<u64>::from(recomputed_total_rewards), Ratio::<u64>::from(observed_total_rewards), "total rewards do not match at era {}", era);
                assert_eq!(Ratio::<u64>::from(recomputed_total_rewards), recomputed_total_supply.get(era).expect("expected recalculated supply") - recomputed_total_supply.get(&(era - &1)).expect("expected recalculated supply"), "supply growth does not match rewards at era {}", era)
            }
        }
        )

}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_small_prime_five_eras() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        PRIME_STAKES,
        5,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_ZERO,
        FINALITY_SIG_PROP_ONE,
        FINALITY_SIG_LOOKBACK,
        REPRESENTATIVE_NODE_INDEX,
        &[]
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_small_prime_five_eras_no_lookback() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        PRIME_STAKES,
        5,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_ZERO,
        FINALITY_SIG_PROP_ONE,
        0u64,
        REPRESENTATIVE_NODE_INDEX,
        &[]
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_no_finality_small_nominal_five_eras() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        &[STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_ZERO,
        FINALITY_SIG_PROP_ZERO,
        FINALITY_SIG_LOOKBACK,
        REPRESENTATIVE_NODE_INDEX,
        &[]
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_half_finality_half_finders_small_nominal_five_eras() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        &[STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_HALF,
        FINALITY_SIG_PROP_HALF,
        FINALITY_SIG_LOOKBACK,
        REPRESENTATIVE_NODE_INDEX,
        &[]
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_half_finality_half_finders_small_nominal_five_eras_no_lookback() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        &[STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_HALF,
        FINALITY_SIG_PROP_HALF,
        0u64,
        REPRESENTATIVE_NODE_INDEX,
        &[]
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders_small_nominal_five_eras_no_lookback() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        &[STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_HALF,
        FINALITY_SIG_PROP_ONE,
        0u64,
        REPRESENTATIVE_NODE_INDEX,
        &[]
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        &[STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE],
        ERA_COUNT,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_HALF,
        FINALITY_SIG_PROP_ONE,
        FINALITY_SIG_LOOKBACK,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_half_finders_five_eras() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        &[STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE],
        5,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_HALF,
        FINALITY_SIG_PROP_ONE,
        FINALITY_SIG_LOOKBACK,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES
    ).await;
}


#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_zug_all_finality_zero_finders() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_ZUG,
        &[STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE],
        ERA_COUNT,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_ZERO,
        FINALITY_SIG_PROP_ONE,
        FINALITY_SIG_LOOKBACK,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_highway_all_finality_zero_finders() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_HIGHWAY,
        &[STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE],
        ERA_COUNT,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_ZERO,
        FINALITY_SIG_PROP_ONE,
        FINALITY_SIG_LOOKBACK,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES
    ).await;
}

#[tokio::test]
#[cfg_attr(not(feature = "failpoints"), ignore)]
async fn run_reward_network_highway_no_finality() {
    run_rewards_network_scenario(
        crate::new_rng(),
        CONSENSUS_HIGHWAY,
        &[STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE, STAKE],
        ERA_COUNT,
        ERA_DURATION,
        MIN_HEIGHT,
        BLOCK_TIME,
        TIME_OUT,
        SEIGNIORAGE,
        FINDERS_FEE_ZERO,
        FINALITY_SIG_PROP_ZERO,
        FINALITY_SIG_LOOKBACK,
        REPRESENTATIVE_NODE_INDEX,
        FILTERED_NODES_INDICES
    ).await;
}