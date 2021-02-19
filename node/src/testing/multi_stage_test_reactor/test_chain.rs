use std::{collections::HashSet, sync::Arc, time::Duration};

use log::info;
use num_rational::Ratio;
use rand::Rng;
use tempfile::TempDir;

use casper_execution_engine::shared::motes::Motes;
use casper_types::{PublicKey, SecretKey, U512};

use crate::{
    components::{consensus::EraId, gossiper, small_network, storage, storage::Storage},
    crypto::AsymmetricKeyExt,
    reactor::{validator, Runner},
    testing::{
        self,
        multi_stage_test_reactor::{InitializerReactorConfigWithChainspec, CONFIG_DIR},
        network::{Network, Nodes},
        ConditionCheckReactor, MultiStageTestReactor,
    },
    types::{
        chainspec::{AccountConfig, AccountsConfig},
        BlockHash, Chainspec, NodeId, Timestamp,
    },
    utils::{External, Loadable, WithDir},
    NodeRng,
};

#[derive(Clone)]
struct SecretKeyWithStake {
    secret_key: SecretKey,
    stake: u64,
}

impl PartialEq for SecretKeyWithStake {
    fn eq(&self, other: &Self) -> bool {
        self.stake == other.stake
            && PublicKey::from(&self.secret_key) == PublicKey::from(&other.secret_key)
    }
}

impl Eq for SecretKeyWithStake {}

struct TestChain {
    // Keys that validator instances will use, can include duplicates
    storages: Vec<TempDir>,
    chainspec: Arc<Chainspec>,
    first_node_port: u16,
    network: Network<MultiStageTestReactor>,
}

impl TestChain {
    /// Instantiates a new test chain configuration.
    ///
    /// Generates secret keys for `size` validators and creates a matching chainspec.
    async fn new(size: usize, rng: &mut NodeRng) -> Self {
        assert!(
            size >= 1,
            "Network size must have at least one node (size: {})",
            size
        );
        let first_node_secret_key_with_stake = SecretKeyWithStake {
            secret_key: SecretKey::random(rng),
            stake: rng.gen_range(100, 999),
        };
        let other_secret_keys_with_stakes = {
            let mut other_secret_keys_with_stakes = Vec::new();
            for _ in 1..size {
                let staked_secret_key = SecretKeyWithStake {
                    secret_key: SecretKey::random(rng),
                    stake: rng.gen_range(100, 999),
                };
                other_secret_keys_with_stakes.push(staked_secret_key)
            }
            other_secret_keys_with_stakes
        };

        Self::new_with_keys(
            first_node_secret_key_with_stake,
            other_secret_keys_with_stakes,
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
        rng: &mut NodeRng,
    ) -> Self {
        // Load the `local` chainspec.
        let mut chainspec: Chainspec = Chainspec::from_resources("local");

        // Override accounts with those generated from the keys.
        let genesis_accounts = std::iter::once(&first_node_secret_key_with_stake)
            .chain(other_secret_keys_with_stakes.iter())
            .map(|staked_secret_key| {
                let public_key = PublicKey::from(&staked_secret_key.secret_key);
                AccountConfig::new(
                    public_key,
                    Motes::new(U512::from(rng.gen_range(10000, 99999999))),
                    Motes::new(U512::from(staked_secret_key.stake)),
                )
            })
            .collect();
        chainspec.network_config.accounts_config = AccountsConfig::new(genesis_accounts);

        // Make the genesis timestamp 45 seconds from now, to allow for all validators to start up.
        chainspec.network_config.timestamp = Timestamp::now() + 45000.into();

        chainspec.core_config.minimum_era_height = 4;
        chainspec.highway_config.finality_threshold_fraction = Ratio::new(34, 100);
        chainspec.core_config.era_duration = 10.into();

        // Assign a port for the first node (TODO: this has a race condition)
        let first_node_port = testing::unused_port_on_localhost();

        // Create the test network
        let network: Network<MultiStageTestReactor> = Network::new();

        let mut test_chain = TestChain {
            chainspec: Arc::new(chainspec),
            storages: Vec::new(),
            first_node_port,
            network,
        };

        // Add the nodes to the chain
        test_chain
            .add_node(true, first_node_secret_key_with_stake.secret_key, None, rng)
            .await;

        for secret_key_with_stake in other_secret_keys_with_stakes {
            test_chain
                .add_node(false, secret_key_with_stake.secret_key, None, rng)
                .await;
        }

        test_chain
    }

    /// Creates an initializer/validator configuration for the `idx`th validator.
    async fn add_node(
        &mut self,
        first_node: bool,
        secret_key: SecretKey,
        trusted_hash: Option<BlockHash>,
        rng: &mut NodeRng,
    ) -> NodeId {
        // Set the network configuration.
        let network = if first_node {
            small_network::Config::default_local_net_first_node(self.first_node_port)
        } else {
            small_network::Config::default_local_net(self.first_node_port)
        };

        let mut validator_config = validator::Config {
            network,
            gossip: gossiper::Config::new_with_small_timeouts(),
            ..Default::default()
        };

        // ...and the secret key for our validator.
        validator_config.consensus.secret_key_path = External::from_value(secret_key);

        // Set a trust hash if one has been provided.
        validator_config.node.trusted_hash = trusted_hash;

        // Additionally set up storage in a temporary directory.
        let (storage_config, temp_dir) = storage::Config::default_for_tests();
        validator_config.consensus.unit_hashes_folder = temp_dir.path().to_path_buf();
        self.storages.push(temp_dir);
        validator_config.storage = storage_config;

        // Bundle our config with a chainspec for creating a multi-stage reactor
        let config = InitializerReactorConfigWithChainspec {
            config: WithDir::new(&*CONFIG_DIR, validator_config),
            chainspec: Arc::clone(&self.chainspec),
        };

        // Add the node (a multi-stage reactor) with the specified config to the network
        self.network
            .add_node_with_config(config, rng)
            .await
            .expect("could not add node to reactor")
            .0
    }
}

/// Get the set of era IDs from a runner.
fn era_ids(runner: &Runner<ConditionCheckReactor<MultiStageTestReactor>>) -> HashSet<EraId> {
    if let Some(consensus) = runner.reactor().inner().consensus() {
        consensus.active_eras().keys().cloned().collect()
    } else {
        HashSet::new()
    }
}

/// Given an era number, return a predicate to check if all of the nodes are in that specified era.
fn is_in_era(era_num: usize) -> impl Fn(&Nodes<MultiStageTestReactor>) -> bool {
    move |nodes: &Nodes<MultiStageTestReactor>| {
        // check ee's era_validators against consensus' validators
        let first_node = nodes.values().next().expect("need at least one node");

        // Get a list of eras from the first node.
        let expected_eras = era_ids(&first_node);

        // Return if not in expected era yet.
        if expected_eras.len() <= era_num {
            return false;
        }

        // Ensure eras are all the same for all other nodes.
        nodes
            .values()
            .map(era_ids)
            .all(|eras| eras == expected_eras)
    }
}

#[tokio::test]
async fn run_validator_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    // Instantiate a new chain with a fixed size.
    const NETWORK_SIZE: usize = 5;
    let mut chain = TestChain::new(NETWORK_SIZE, &mut rng).await;

    // Wait for all nodes to agree on one era.
    for era_num in 1..=2 {
        info!("Waiting for Era {} to end", era_num);
        chain
            .network
            .settle_on(&mut rng, is_in_era(era_num), Duration::from_secs(600))
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
        secret_key: SecretKey::random(&mut rng),
        stake: 100,
    };
    let alice_sk = SecretKeyWithStake {
        secret_key: SecretKey::random(&mut rng),
        stake: 1,
    };
    let other_secret_keys_with_stakes = vec![alice_sk.clone(), alice_sk];

    let mut chain = TestChain::new_with_keys(
        first_node_secret_key_with_stake,
        other_secret_keys_with_stakes,
        &mut rng,
    )
    .await;

    for era_num in 1..=5 {
        info!("Waiting for Era {} to end", era_num);
        chain
            .network
            .settle_on(&mut rng, is_in_era(era_num), Duration::from_secs(600))
            .await;
    }
}

async fn get_switch_block_hash(
    switch_block_era_num: usize,
    net: &mut Network<MultiStageTestReactor>,
    rng: &mut NodeRng,
) -> BlockHash {
    let era_after_switch_block_era_num = switch_block_era_num + 1;
    info!("Waiting for Era {} to end", era_after_switch_block_era_num);
    net.settle_on(
        rng,
        is_in_era(era_after_switch_block_era_num),
        Duration::from_secs(600),
    )
    .await;

    info!(
        "Querying storage for switch block for Era {}",
        switch_block_era_num
    );

    // Get the storage for the first reactor
    let storage: &Storage = net
        .nodes()
        .values()
        .next()
        .expect("need at least one node")
        .reactor()
        .inner()
        .storage()
        .expect("Can not access storage of first node");
    let switch_block = storage
        .transactional_get_switch_block_by_era_id(switch_block_era_num as u64)
        .expect("Could not find block for era num");
    let switch_block_hash = switch_block.hash();
    info!(
        "Found block hash for Era {}: {}",
        switch_block_era_num, switch_block_hash
    );
    *switch_block_hash
}

// TODO remove ignore once joiner test is consistent again.
#[ignore]
#[tokio::test]
async fn test_joiner() {
    testing::init_logging();

    const INITIAL_NETWORK_SIZE: usize = 1;

    let mut rng = crate::new_rng();

    // Create a chain with just one node
    let mut chain = TestChain::new(INITIAL_NETWORK_SIZE, &mut rng).await;

    assert_eq!(
        chain.network.nodes().len(),
        INITIAL_NETWORK_SIZE,
        "There should be just one bonded validator in the network"
    );

    // Get the first switch block hash
    let first_switch_block_hash = get_switch_block_hash(1, &mut chain.network, &mut rng).await;

    // Have a node join the network with that hash
    info!("Joining with trusted hash {}", first_switch_block_hash);
    let joiner_node_secret_key = SecretKey::random(&mut rng);
    chain
        .add_node(
            false,
            joiner_node_secret_key,
            Some(first_switch_block_hash),
            &mut rng,
        )
        .await;

    assert_eq!(
        chain.network.nodes().len(),
        2,
        "There should be two validators in the network (one bonded and one read only)"
    );

    let era_num = 3;
    info!("Waiting for Era {} to end", era_num);
    chain
        .network
        .settle_on(&mut rng, is_in_era(era_num), Duration::from_secs(600))
        .await;
}
