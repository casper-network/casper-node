use std::{sync::Arc, time::Duration};

use log::info;
use num::Zero;
use num_rational::Ratio;
use rand::Rng;
use tempfile::TempDir;

use casper_types::{
    system::auction::DelegationRate, EraId, Motes, PublicKey, SecretKey, Timestamp, U512,
};

use crate::{
    components::{gossiper, small_network, storage, storage::Storage},
    reactor::participating,
    testing::{
        self,
        multi_stage_test_reactor::{InitializerReactorConfigWithChainspec, CONFIG_DIR},
        network::{Network, Nodes},
        MultiStageTestReactor,
    },
    types::{
        chainspec::{AccountConfig, AccountsConfig, ValidatorConfig},
        ActivationPoint, BlockHash, BlockHeader, Chainspec, ChainspecRawBytes, NodeId,
    },
    utils::{External, Loadable, WithDir},
    NodeRng,
};

#[derive(Clone)]
struct SecretKeyWithStake {
    secret_key: Arc<SecretKey>,
    stake: u64,
}

impl PartialEq for SecretKeyWithStake {
    fn eq(&self, other: &Self) -> bool {
        self.stake == other.stake
            && PublicKey::from(&*self.secret_key) == PublicKey::from(&*other.secret_key)
    }
}

impl Eq for SecretKeyWithStake {}

struct TestChain {
    // Keys that validator instances will use, can include duplicates
    storages: Vec<TempDir>,
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,
    first_node_port: u16,
    network: Network<MultiStageTestReactor>,
}

impl TestChain {
    /// Instantiates a new test chain configuration.
    ///
    /// Generates secret keys for `size` validators and creates a matching chainspec.
    async fn new(
        size: usize,
        verifiable_chunked_hash_activation: EraId,
        rng: &mut NodeRng,
    ) -> Self {
        assert!(
            size >= 1,
            "Network size must have at least one node (size: {})",
            size
        );
        let first_node_secret_key_with_stake = SecretKeyWithStake {
            secret_key: Arc::new(SecretKey::random(rng)),
            stake: rng.gen_range(100..999),
        };
        let other_secret_keys_with_stakes = {
            let mut other_secret_keys_with_stakes = Vec::new();
            for _ in 1..size {
                let staked_secret_key = SecretKeyWithStake {
                    secret_key: Arc::new(SecretKey::random(rng)),
                    stake: rng.gen_range(100..999),
                };
                other_secret_keys_with_stakes.push(staked_secret_key)
            }
            other_secret_keys_with_stakes
        };

        Self::new_with_keys(
            first_node_secret_key_with_stake,
            other_secret_keys_with_stakes,
            verifiable_chunked_hash_activation,
            rng,
        )
        .await
    }

    /// Instantiates a new test chain configuration.
    ///
    /// Takes a vector of bonded keys with specified bond amounts.
    async fn new_with_keys(
        first_node_secret_key_with_stake: SecretKeyWithStake,
        other_secret_keys_with_stakes: Vec<SecretKeyWithStake>,
        verifiable_chunked_hash_activation: EraId,
        rng: &mut NodeRng,
    ) -> Self {
        // Load the `local` chainspec.
        let (mut chainspec, chainspec_raw_bytes) =
            <(Chainspec, ChainspecRawBytes)>::from_resources("local");

        // Override accounts with those generated from the keys.
        let genesis_accounts = std::iter::once(&first_node_secret_key_with_stake)
            .chain(other_secret_keys_with_stakes.iter())
            .map(|staked_secret_key| {
                let public_key = PublicKey::from(&*staked_secret_key.secret_key);
                let validator_config = ValidatorConfig::new(
                    Motes::new(U512::from(staked_secret_key.stake)),
                    DelegationRate::zero(),
                );
                AccountConfig::new(
                    public_key,
                    Motes::new(U512::from(rng.gen_range(10000..99999999))),
                    Some(validator_config),
                )
            })
            .collect();
        let delegators = vec![];
        chainspec.network_config.accounts_config =
            AccountsConfig::new(genesis_accounts, delegators);

        // Make the genesis timestamp 45 seconds from now, to allow for all validators to start up.
        chainspec.protocol_config.activation_point =
            ActivationPoint::Genesis(Timestamp::now() + 45000.into());

        chainspec.core_config.minimum_era_height = 4;
        chainspec.highway_config.finality_threshold_fraction = Ratio::new(34, 100);
        chainspec.core_config.era_duration = 10.into();
        chainspec.core_config.auction_delay = 1;
        chainspec.core_config.unbonding_delay = 3;

        chainspec.protocol_config.verifiable_chunked_hash_activation =
            verifiable_chunked_hash_activation;

        // Assign a port for the first node (TODO: this has a race condition)
        let first_node_port = testing::unused_port_on_localhost();

        // Create the test network
        let network: Network<MultiStageTestReactor> = Network::new();

        let mut test_chain = TestChain {
            storages: Vec::new(),
            chainspec: Arc::new(chainspec),
            chainspec_raw_bytes: Arc::new(chainspec_raw_bytes),
            first_node_port,
            network,
        };

        // Add the nodes to the chain
        test_chain
            .add_node(
                true,
                first_node_secret_key_with_stake.secret_key,
                None,
                false,
                rng,
            )
            .await;

        for secret_key_with_stake in other_secret_keys_with_stakes {
            test_chain
                .add_node(false, secret_key_with_stake.secret_key, None, false, rng)
                .await;
        }

        test_chain
    }

    /// Creates an initializer/validator configuration for the `idx`th validator.
    async fn add_node(
        &mut self,
        first_node: bool,
        secret_key: Arc<SecretKey>,
        trusted_hash: Option<BlockHash>,
        sync_to_genesis: bool,
        rng: &mut NodeRng,
    ) -> NodeId {
        // Set the network configuration.
        let network = if first_node {
            small_network::Config::default_local_net_first_node(self.first_node_port)
        } else {
            small_network::Config::default_local_net(self.first_node_port)
        };

        let mut participating_config = participating::Config {
            network,
            gossip: gossiper::Config::new_with_small_timeouts(),
            ..Default::default()
        };

        participating_config.node.sync_to_genesis = sync_to_genesis;

        // Additionally set up storage in a temporary directory.
        let (storage_config, temp_dir) = storage::Config::default_for_tests();
        // ...and the secret key for our validator.
        {
            let secret_key_path = temp_dir.path().join("secret_key");
            secret_key
                .to_file(secret_key_path.clone())
                .expect("could not write secret key");
            participating_config.consensus.secret_key_path = External::Path(secret_key_path);
        }
        self.storages.push(temp_dir);
        participating_config.storage = storage_config;

        // Set a trust hash if one has been provided.
        participating_config.node.trusted_hash = trusted_hash;

        // Bundle our config with a chainspec for creating a multi-stage reactor
        let config = InitializerReactorConfigWithChainspec {
            config: WithDir::new(&*CONFIG_DIR, participating_config),
            chainspec: Arc::clone(&self.chainspec),
            chainspec_raw_bytes: Arc::clone(&self.chainspec_raw_bytes),
        };

        // Add the node (a multi-stage reactor) with the specified config to the network
        self.network
            .add_node_with_config(config, rng)
            .await
            .expect("could not add node to reactor")
            .0
    }
}

/// Given an era number, returns a predicate to check if all of the nodes are in the specified era
/// or have moved forward.
fn has_passed_by_era(era_num: u64) -> impl Fn(&Nodes<MultiStageTestReactor>) -> bool {
    move |nodes: &Nodes<MultiStageTestReactor>| {
        let era_id = EraId::from(era_num);
        nodes.values().all(|runner| {
            runner
                .reactor()
                .inner()
                .consensus()
                .map_or(false, |consensus| consensus.current_era() >= era_id)
        })
    }
}

/// Check that all nodes have downloaded the state root under the genesis block.
fn all_have_genesis_state(nodes: &Nodes<MultiStageTestReactor>) -> bool {
    nodes.values().all(|runner| {
        let reactor = runner.reactor().inner();
        let genesis_state_root = if let Some(genesis_state_root) = reactor
            .storage()
            .and_then(|storage| {
                storage
                    .read_block_header_by_height(0)
                    .expect("Could not read block at height 0")
            })
            .map(|genesis_block_header| *genesis_block_header.state_root_hash())
        {
            genesis_state_root
        } else {
            return false;
        };
        reactor
            .contract_runtime()
            .map_or(false, move |contract_runtime| {
                contract_runtime
                    .retrieve_trie_keys(vec![genesis_state_root])
                    .expect("Could not read DB")
                    .is_empty()
            })
    })
}

#[tokio::test]
async fn run_participating_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    // `verifiable_chunked_hash_activation` can be chosen arbitrarily
    let verifiable_chunked_hash_activation = EraId::from(rng.gen_range(0..=10));

    // Instantiate a new chain with a fixed size.
    const NETWORK_SIZE: usize = 5;
    let mut chain =
        TestChain::new(NETWORK_SIZE, verifiable_chunked_hash_activation, &mut rng).await;

    // Wait for all nodes to agree on one era.
    for era_num in 1..=2 {
        info!("Waiting for Era {} to end", era_num);
        chain
            .network
            .settle_on(
                &mut rng,
                has_passed_by_era(era_num),
                Duration::from_secs(600),
            )
            .await;
    }
}

#[tokio::test]
async fn run_equivocator_network() {
    // Test that we won't panic if a node equivocates
    // Creates an equivocating node by launching two reactors with the same private key (alice_sk)
    // The two nodes will create signatures of distinct units causing an equivocation
    testing::init_logging();

    let mut rng: NodeRng = crate::new_rng();

    let first_node_secret_key_with_stake = SecretKeyWithStake {
        secret_key: Arc::new(SecretKey::random(&mut rng)),
        stake: 100,
    };
    let alice_sk = SecretKeyWithStake {
        secret_key: Arc::new(
            SecretKey::ed25519_from_bytes([0; SecretKey::ED25519_LENGTH]).unwrap(),
        ),
        stake: 1,
    };

    let other_secret_keys_with_stakes = vec![alice_sk.clone(), alice_sk];

    // `verifiable_chunked_hash_activation` can be chosen arbitrarily
    let verifiable_chunked_hash_activation = EraId::from(rng.gen_range(0..=10));

    let mut chain = TestChain::new_with_keys(
        first_node_secret_key_with_stake,
        other_secret_keys_with_stakes,
        verifiable_chunked_hash_activation,
        &mut rng,
    )
    .await;

    for era_num in 1..=5 {
        info!("Waiting for Era {} to end", era_num);
        chain
            .network
            .settle_on(
                &mut rng,
                has_passed_by_era(era_num),
                Duration::from_secs(600),
            )
            .await;
    }
}

fn first_node_storage(net: &Network<MultiStageTestReactor>) -> &Storage {
    net.nodes()
        .values()
        .next()
        .expect("need at least one node")
        .reactor()
        .inner()
        .storage()
        .expect("Can not access storage of first node")
}

async fn await_switch_block(
    switch_block_era_num: u64,
    verifiable_chunked_hash_activation: EraId,
    net: &mut Network<MultiStageTestReactor>,
    rng: &mut NodeRng,
) -> BlockHeader {
    let era_after_switch_block_era_num = switch_block_era_num + 1;
    info!("Waiting for Era {} to end", era_after_switch_block_era_num);
    net.settle_on(
        rng,
        has_passed_by_era(era_after_switch_block_era_num),
        Duration::from_secs(600),
    )
    .await;

    info!(
        "Querying storage for switch block for Era {}",
        switch_block_era_num
    );

    // Get the storage for the first reactor
    let switch_block_header = first_node_storage(net)
        .read_switch_block_header_by_era_id(EraId::from(switch_block_era_num))
        .expect("Storage error")
        .expect("Could not find block for era num");
    info!(
        "Found block hash for Era {}: {:?}",
        switch_block_era_num,
        switch_block_header.hash(verifiable_chunked_hash_activation)
    );
    switch_block_header
}

/// Test a node joining to a single node network at genesis
#[tokio::test]
async fn test_joiner_at_genesis() {
    testing::init_logging();

    const INITIAL_NETWORK_SIZE: usize = 1;

    let mut rng = crate::new_rng();

    // `verifiable_chunked_hash_activation` can be chosen arbitrarily
    let verifiable_chunked_hash_activation = EraId::from(rng.gen_range(0..=10));

    // Create a chain with just one node
    let mut chain = TestChain::new(
        INITIAL_NETWORK_SIZE,
        verifiable_chunked_hash_activation,
        &mut rng,
    )
    .await;

    assert_eq!(
        chain.network.nodes().len(),
        INITIAL_NETWORK_SIZE,
        "There should be just one bonded validator in the network"
    );

    // Get the first switch block hash
    // As part of the chain sync process, we will need to retrieve the first switch block
    let start_era = 2;
    let _ = await_switch_block(
        start_era,
        verifiable_chunked_hash_activation,
        &mut chain.network,
        &mut rng,
    )
    .await;

    let trusted_hash = first_node_storage(&chain.network)
        .read_block_header_by_height(2)
        .expect("should not have storage error")
        .expect("should have block header")
        .hash(verifiable_chunked_hash_activation);

    // Have a node join the network with that hash
    info!("Joining with trusted hash {}", trusted_hash);
    let joiner_node_secret_key = Arc::new(SecretKey::random(&mut rng));
    chain
        .add_node(
            false,
            joiner_node_secret_key,
            Some(trusted_hash),
            false,
            &mut rng,
        )
        .await;

    assert_eq!(
        chain.network.nodes().len(),
        2,
        "There should be two validators in the network (one bonded and one read only)"
    );

    let end_era = start_era + 1;
    info!("Waiting for Era {} to end", end_era);
    chain
        .network
        .settle_on(
            &mut rng,
            has_passed_by_era(end_era),
            Duration::from_secs(600),
        )
        .await;
}

/// Test a node joining to a single node network
#[tokio::test]
async fn test_sync_to_genesis() {
    testing::init_logging();

    const INITIAL_NETWORK_SIZE: usize = 1;

    let mut rng = crate::new_rng();

    const ERA_TO_JOIN: u64 = 3;

    // We need to make sure we're in the Merkle-based hashing scheme.
    let verifiable_chunked_hash_activation = EraId::from(ERA_TO_JOIN - 1);

    // Create a chain with just one node
    let mut chain = TestChain::new(
        INITIAL_NETWORK_SIZE,
        verifiable_chunked_hash_activation,
        &mut rng,
    )
    .await;

    assert_eq!(
        chain.network.nodes().len(),
        INITIAL_NETWORK_SIZE,
        "There should be just one bonded validator in the network"
    );

    // Get the first switch block hash
    // As part of the chain sync process, we will need to retrieve the first switch block
    let switch_block_hash = await_switch_block(
        1,
        verifiable_chunked_hash_activation,
        &mut chain.network,
        &mut rng,
    )
    .await
    .hash(verifiable_chunked_hash_activation);

    info!("Waiting for Era {} to end", ERA_TO_JOIN);
    chain
        .network
        .settle_on(
            &mut rng,
            has_passed_by_era(ERA_TO_JOIN),
            Duration::from_secs(600),
        )
        .await;

    // Have a node join the network with that hash
    info!("Joining with trusted hash {}", switch_block_hash);
    let joiner_node_secret_key = Arc::new(SecretKey::random(&mut rng));
    chain
        .add_node(
            false,
            joiner_node_secret_key,
            Some(switch_block_hash),
            true,
            &mut rng,
        )
        .await;

    assert_eq!(
        chain.network.nodes().len(),
        2,
        "There should be two nodes in the network (one bonded validator and one read only)"
    );

    let synchronized_era = ERA_TO_JOIN + 1;
    info!("Waiting for Era {} to end", synchronized_era);
    chain
        .network
        .settle_on(
            &mut rng,
            has_passed_by_era(synchronized_era),
            Duration::from_secs(600),
        )
        .await;

    chain
        .network
        .settle_on(&mut rng, all_have_genesis_state, Duration::from_secs(600))
        .await;

    for node in chain.network.nodes().values() {
        let reactor = node.reactor().inner();
        let storage = reactor.storage().expect("must have storage");
        let highest_block_header = storage
            .read_highest_block_header()
            .expect("must read from storage")
            .expect("must have highest block header");
        // Check every block and its state root going back to genesis
        let mut block_hash = highest_block_header.hash(verifiable_chunked_hash_activation);
        loop {
            let block = storage
                .read_block(&block_hash)
                .expect("must read from storage")
                .expect("must have block");
            let missing_tries = reactor
                .contract_runtime()
                .expect("must have contract runtime")
                .retrieve_trie_keys(vec![*block.state_root_hash()])
                .expect("must read tries");
            assert_eq!(missing_tries, vec![], "should be no missing tries");
            if let Some(parent_hash) = block.parent() {
                block_hash = *parent_hash;
            } else {
                break;
            }
        }
    }
}

/// Test a node joining to a single node network
#[tokio::test]
async fn test_joiner() {
    testing::init_logging();

    const INITIAL_NETWORK_SIZE: usize = 1;

    let mut rng = crate::new_rng();

    const ERA_TO_JOIN: u64 = 3;

    // `verifiable_chunked_hash_activation` can be chosen arbitrarily.
    // Ideally, we should run this test two times with `verifiable_chunked_hash_activation`
    // set to "before" and "after" the `ERA_TO_JOIN`. But because this test is
    // time consuming, we randomize the activation point so it randomly falls
    // ahead or behind the era.
    let verifiable_chunked_hash_activation: u64 = rng.gen_range(0..=(ERA_TO_JOIN * 2));
    let verifiable_chunked_hash_activation = EraId::from(verifiable_chunked_hash_activation);

    // Create a chain with just one node
    let mut chain = TestChain::new(
        INITIAL_NETWORK_SIZE,
        verifiable_chunked_hash_activation,
        &mut rng,
    )
    .await;

    assert_eq!(
        chain.network.nodes().len(),
        INITIAL_NETWORK_SIZE,
        "There should be just one bonded validator in the network"
    );

    // Get the first switch block hash
    // As part of the chain sync process, we will need to retrieve the first switch block
    let switch_block_hash = await_switch_block(
        1,
        verifiable_chunked_hash_activation,
        &mut chain.network,
        &mut rng,
    )
    .await
    .hash(verifiable_chunked_hash_activation);

    info!("Waiting for Era {} to end", ERA_TO_JOIN);
    chain
        .network
        .settle_on(
            &mut rng,
            has_passed_by_era(ERA_TO_JOIN),
            Duration::from_secs(600),
        )
        .await;

    // Have a node join the network with that hash
    info!("Joining with trusted hash {}", switch_block_hash);
    let joiner_node_secret_key = Arc::new(SecretKey::random(&mut rng));
    chain
        .add_node(
            false,
            joiner_node_secret_key,
            Some(switch_block_hash),
            false,
            &mut rng,
        )
        .await;

    assert_eq!(
        chain.network.nodes().len(),
        2,
        "There should be two validators in the network (one bonded and one read only)"
    );

    let synchronized_era = ERA_TO_JOIN + 1;
    info!("Waiting for Era {} to end", synchronized_era);
    chain
        .network
        .settle_on(
            &mut rng,
            has_passed_by_era(synchronized_era),
            Duration::from_secs(600),
        )
        .await;
}

/// Test a node joining to a network with five nodes
#[tokio::test]
async fn test_joiner_network() {
    testing::init_logging();

    const INITIAL_NETWORK_SIZE: usize = 5;

    let mut rng = crate::new_rng();

    const START_ERA: u64 = 2;

    // `verifiable_chunked_hash_activation` can be chosen arbitrarily.
    // Ideally, we should run this test two times with `verifiable_chunked_hash_activation`
    // set to "before" and "after" the `ERA_TO_JOIN`. But because this test is
    // time consuming, we randomize the activation point so it randomly falls
    // ahead or behind the era.
    let verifiable_chunked_hash_activation: u64 = rng.gen_range(0..=(START_ERA * 2));
    let verifiable_chunked_hash_activation = EraId::from(verifiable_chunked_hash_activation);

    let mut chain = TestChain::new(
        INITIAL_NETWORK_SIZE,
        verifiable_chunked_hash_activation,
        &mut rng,
    )
    .await;

    assert_eq!(
        chain.network.nodes().len(),
        INITIAL_NETWORK_SIZE,
        "Wrong number of bonded validators in the network"
    );

    // Get the first switch block hash
    await_switch_block(
        START_ERA,
        verifiable_chunked_hash_activation,
        &mut chain.network,
        &mut rng,
    )
    .await;

    let trusted_block_hash = first_node_storage(&chain.network)
        .read_block_header_by_height(2)
        .expect("should not have storage error")
        .expect("should have block header")
        .hash(verifiable_chunked_hash_activation);

    // Have a node join the network with that hash
    info!("Joining with trusted hash {}", trusted_block_hash);
    let joiner_node_secret_key = Arc::new(SecretKey::random(&mut rng));
    chain
        .add_node(
            false,
            joiner_node_secret_key,
            Some(trusted_block_hash),
            false,
            &mut rng,
        )
        .await;

    assert_eq!(chain.network.nodes().len(), INITIAL_NETWORK_SIZE + 1,);

    const END_ERA: u64 = START_ERA + 1;
    info!("Waiting for Era {} to end", END_ERA);
    chain
        .network
        .settle_on(
            &mut rng,
            has_passed_by_era(END_ERA),
            Duration::from_secs(900),
        )
        .await;
}
