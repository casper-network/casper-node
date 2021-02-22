use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::bail;
use log::info;
use num_rational::Ratio;
use rand::Rng;
use tempfile::TempDir;

use casper_execution_engine::shared::motes::Motes;
use casper_types::{PublicKey, SecretKey, U512};

use crate::{
    components::{consensus::EraId, gossiper, small_network, storage},
    crypto::AsymmetricKeyExt,
    reactor::{initializer, joiner, validator, ReactorExit, Runner},
    testing::{self, network::Network, ConditionCheckReactor, TestRng},
    types::{
        chainspec::{AccountConfig, AccountsConfig},
        Chainspec, Timestamp,
    },
    utils::{External, Loadable, WithDir, RESOURCES_PATH},
    NodeRng,
};

struct TestChain {
    // Keys that validator instances will use, can include duplicates
    keys: Vec<SecretKey>,
    storages: Vec<TempDir>,
    chainspec: Arc<Chainspec>,
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
        let mut chainspec = Chainspec::from_resources("local");

        // Override accounts with those generated from the keys.
        let accounts = stakes
            .iter()
            .map(|(public_key, bounded_amounts_u64)| {
                AccountConfig::new(
                    *public_key,
                    Motes::new(U512::from(rng.gen_range(10000, 99999999))),
                    Motes::new(U512::from(*bounded_amounts_u64)),
                )
            })
            .collect();

        chainspec.network_config.accounts_config = AccountsConfig::new(accounts);

        // Make the genesis timestamp 45 seconds from now, to allow for all validators to start up.
        chainspec.network_config.timestamp = Timestamp::now() + 45000.into();

        chainspec.core_config.minimum_era_height = 1;
        chainspec.highway_config.finality_threshold_fraction = Ratio::new(34, 100);
        chainspec.core_config.era_duration = 10.into();

        TestChain {
            keys,
            chainspec: Arc::new(chainspec),
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
            gossip: gossiper::Config::new_with_small_timeouts(),
            ..Default::default()
        };

        // ...and the secret key for our validator.
        cfg.consensus.secret_key_path = External::from_value(self.keys[idx].duplicate());

        // Additionally set up storage in a temporary directory.
        let (storage_cfg, temp_dir) = storage::Config::default_for_tests();
        cfg.consensus.unit_hashes_folder = temp_dir.path().to_path_buf();
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
            let mut initializer_runner = Runner::<initializer::Reactor>::new_with_chainspec(
                WithDir::new(root.clone(), cfg),
                Arc::clone(&self.chainspec),
            )
            .await?;
            let reactor_exit = initializer_runner.run(rng).await;
            if reactor_exit != ReactorExit::ProcessShouldContinue {
                bail!("failed to initialize successfully");
            }

            // Now we can construct the actual node.
            let initializer = initializer_runner.into_inner();
            let mut joiner_runner =
                Runner::<joiner::Reactor>::new(WithDir::new(root.clone(), initializer), rng)
                    .await?;
            let _ = joiner_runner.run(rng).await;

            let config = joiner_runner.into_inner().into_validator_config().await?;

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

// TODO: fix this test
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
