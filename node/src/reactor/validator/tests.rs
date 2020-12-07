use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

use anyhow::bail;
use rand::Rng;
use tempfile::TempDir;

use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use casper_types::U512;
use log::info;

use crate::{
    components::{consensus::EraId, small_network, storage},
    crypto::asymmetric_key::{PublicKey, SecretKey},
    reactor::{initializer, joiner, validator, Runner},
    testing::{self, network::Network, ConditionCheckReactor, TestRng},
    types::Timestamp,
    utils::{External, Loadable, WithDir, RESOURCES_PATH},
    Chainspec, NodeRng,
};

struct TestChain {
    // Keys that validator instances will use, can include duplicates
    keys: Vec<SecretKey>,
    storages: Vec<TempDir>,
    chainspec: Chainspec,
}

type Nodes = crate::testing::network::Nodes<validator::Reactor>;

impl TestChain {
    /// Instantiates a new test chain configuration.
    ///
    /// Generates secret keys for `size` validators and creates a matching chainspec.
    fn new(rng: &mut TestRng, size: usize) -> Self {
        let keys: Vec<SecretKey> = (0..size).map(|_| SecretKey::random(rng)).collect();
        let stakes = keys
            .iter()
            .map(|secret_key| (PublicKey::from(secret_key), rng.gen_range(100, 999)))
            .collect();
        Self::new_with_keys(rng, keys, stakes)
    }

    /// Instantiates a new test chain configuration.
    ///
    /// Takes a vector of bonded keys with specified bond amounts.
    fn new_with_keys(
        rng: &mut TestRng,
        keys: Vec<SecretKey>,
        stakes: BTreeMap<PublicKey, u64>,
    ) -> Self {
        // Load the `local` chainspec.
        let mut chainspec: Chainspec = Chainspec::from_resources("local/chainspec.toml");

        // Override accounts with those generated from the keys.
        chainspec.genesis.accounts = stakes
            .iter()
            .map(|(public_key, bounded_amounts_u64)| {
                GenesisAccount::new(
                    (*public_key).into(),
                    public_key.to_account_hash(),
                    Motes::new(U512::from(rng.gen_range(10000, 99999999))),
                    Motes::new(U512::from(*bounded_amounts_u64)),
                )
            })
            .collect();
        // TODO: This is duplicated. Remove the `HighwayConfig` field.
        // Make the genesis timestamp 45 seconds from now, to allow for all validators to start up.
        chainspec.genesis.timestamp = Timestamp::now() + 45000.into();
        chainspec.genesis.highway_config.genesis_era_start_timestamp = chainspec.genesis.timestamp;

        chainspec.genesis.highway_config.minimum_era_height = 1;
        chainspec.genesis.highway_config.finality_threshold_percent = 34;
        chainspec.genesis.highway_config.era_duration = 10.into();

        TestChain {
            keys,
            chainspec,
            storages: Vec::new(),
        }
    }

    /// Creates an initializer/validator configuration for the `idx`th validator.
    fn create_node_config(&mut self, idx: usize, first_node_port: u16) -> validator::Config {
        // Set the network configuration.
        let mut cfg = validator::Config {
            network: if idx == 0 {
                small_network::Config::default_local_net_first_node(first_node_port)
            } else {
                small_network::Config::default_local_net(first_node_port)
            },
            ..Default::default()
        };

        // Set the correct chainspec...
        cfg.node.chainspec_config_path = External::value(self.chainspec.clone());

        // ...and the secret key for our validator.
        cfg.consensus.secret_key_path = External::value(self.keys[idx].duplicate());

        // Additionally set up storage in a temporary directory.
        let (storage_cfg, temp_dir) = storage::Config::default_for_tests();
        self.storages.push(temp_dir);
        cfg.storage = storage_cfg;

        cfg
    }

    async fn create_initialized_network(
        &mut self,
        rng: &mut NodeRng,
    ) -> anyhow::Result<Network<validator::Reactor>> {
        let root = RESOURCES_PATH.join("local");

        let mut network: Network<validator::Reactor> = Network::new();
        let first_node_port = testing::unused_port_on_localhost();

        for idx in 0..self.keys.len() {
            let cfg = self.create_node_config(idx, first_node_port);

            // We create an initializer reactor here and run it to completion.
            let mut initializer_runner =
                Runner::<initializer::Reactor>::new(WithDir::new(root.clone(), cfg), rng).await?;
            initializer_runner.run(rng).await;

            // Now we can construct the actual node.
            let initializer = initializer_runner.into_inner();
            if !initializer.stopped_successfully() {
                bail!("failed to initialize successfully");
            }

            let mut joiner_runner =
                Runner::<joiner::Reactor>::new(WithDir::new(root.clone(), initializer), rng)
                    .await?;
            joiner_runner.run(rng).await;

            let config = joiner_runner.into_inner().into_validator_config().await;

            network
                .add_node_with_config(config, rng)
                .await
                .expect("could not add node to reactor");
        }

        Ok(network)
    }
}

/// Get the set of era IDs from a runner.
fn era_ids(runner: &Runner<ConditionCheckReactor<validator::Reactor>>) -> HashSet<EraId> {
    runner
        .reactor()
        .inner()
        .consensus()
        .active_eras()
        .keys()
        .cloned()
        .collect()
}

/// Given an era number, return a predicate to check if all of the nodes are in that specified era.
fn is_in_era(era_num: usize) -> impl Fn(&Nodes) -> bool {
    move |nodes: &Nodes| {
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
    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE);

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    // Wait for all nodes to agree on one era.
    net.settle_on(&mut rng, is_in_era(1), Duration::from_secs(90))
        .await;

    net.settle_on(&mut rng, is_in_era(2), Duration::from_secs(60))
        .await;
}

#[tokio::test]
async fn run_equivocator_network() {
    testing::init_logging();

    let mut rng = crate::new_rng();

    let alice_sk = SecretKey::random(&mut rng);
    let size: usize = 2;
    let mut keys: Vec<SecretKey> = (1..size).map(|_| SecretKey::random(&mut rng)).collect();
    let mut stakes: BTreeMap<PublicKey, u64> = keys
        .iter()
        .map(|secret_key| (PublicKey::from(secret_key), 100))
        .collect();
    stakes.insert(PublicKey::from(&alice_sk), 1);
    keys.push(alice_sk.clone());
    keys.push(alice_sk);

    let mut chain = TestChain::new_with_keys(&mut rng, keys, stakes);

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    info!("Waiting for Era 1 to end");
    net.settle_on(&mut rng, is_in_era(1), Duration::from_secs(600))
        .await;

    info!("Waiting for Era 2 to end");
    net.settle_on(&mut rng, is_in_era(2), Duration::from_secs(90))
        .await;

    info!("Waiting for Era 3 to end");
    net.settle_on(&mut rng, is_in_era(3), Duration::from_secs(90))
        .await;

    info!("Waiting for Era 4 to end");
    net.settle_on(&mut rng, is_in_era(4), Duration::from_secs(90))
        .await;

    println!("Waiting for Era 5 to end");
    net.settle_on(&mut rng, is_in_era(5), Duration::from_secs(90))
        .await;
}
