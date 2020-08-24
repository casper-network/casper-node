// FIXME: Remove
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unreachable_code)]

use rand::Rng;
use tempfile::TempDir;

use crate::{
    components::{
        consensus::EraId, contract_runtime::core::engine_state::genesis::GenesisConfig,
        in_memory_network::NodeId, storage,
    },
    crypto::asymmetric_key::SecretKey,
    reactor::{initializer, validator, Runner},
    testing::{init_logging, network::Network, ConditionCheckReactor, TestRng},
    types::{Motes, NodeConfig},
    utils::{External, Loadable, WithDir, RESOURCES_PATH},
    Chainspec, GenesisAccount,
};
use anyhow::bail;
use casperlabs_types::U512;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

struct TestChain {
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
        // Create a secret key for each validator.
        let keys: Vec<SecretKey> = (0..size).map(|_| SecretKey::random(rng)).collect();

        // Load the `local` chainspec.
        let mut chainspec = Chainspec::from_resources("local/chainspec.toml");

        // Override accounts with those generated from the keys.
        chainspec.genesis.accounts = keys
            .iter()
            .map(|secret_key| {
                GenesisAccount::with_public_key(
                    secret_key.into(),
                    Motes::new(U512::from(rng.gen_range(10000, 99999999))),
                    Motes::new(U512::from(rng.gen_range(100, 999))),
                )
            })
            .collect();

        TestChain {
            keys,
            chainspec,
            storages: Vec::new(),
        }
    }

    /// Creates an initializer/validator configuration for the `idx`th validator.
    fn create_node_config(&mut self, idx: usize) -> validator::Config {
        // Start with a default configuration.
        let mut cfg = validator::Config::default();

        // We need to set the correct chainspec...
        cfg.node.chainspec_config_path = External::value(self.chainspec.clone());

        // ...and the secret key for our validator.
        cfg.consensus.secret_key_path = External::value(self.keys[idx].duplicate());

        // Additionally set up storage in a temporary directory.
        let (storage_cfg, temp_dir) = storage::Config::default_for_tests();
        self.storages.push(temp_dir);
        cfg.storage = storage_cfg;

        cfg
    }

    async fn create_initialized_network<R: Rng + ?Sized>(
        &mut self,
        rng: &mut R,
    ) -> anyhow::Result<Network<validator::Reactor>> {
        let root = RESOURCES_PATH.join("local");

        let mut network: Network<validator::Reactor> = Network::new();

        for idx in 0..self.keys.len() {
            let cfg = self.create_node_config(idx);

            // We create an initializer reactor here and run it to completion.
            let mut runner =
                Runner::<initializer::Reactor>::new(WithDir::new(root.clone(), cfg), rng).await?;
            runner.run(rng).await;

            // Now we can construct the actual node.
            let initializer = runner.into_inner();
            if !initializer.stopped_successfully() {
                bail!("failed to initialize successfully");
            }

            network
                .add_node_with_config(WithDir::new(root.clone(), initializer), rng)
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

#[tokio::test]
async fn run_validator_network() {
    init_logging();

    let mut rng = TestRng::new();

    // Instantiate a new chain with a fixed size.
    const NETWORK_SIZE: usize = 5;
    let mut chain = TestChain::new(&mut rng, NETWORK_SIZE);

    let mut net = chain
        .create_initialized_network(&mut rng)
        .await
        .expect("network initialization failed");

    let is_in_era = |era_num| {
        move |nodes: &Nodes| {
            let first_node = nodes.values().next().expect("need at least one node");

            // Get a list of eras from the first node.
            let expected_eras = era_ids(&first_node);

            // Return if not in expected era yet.
            if expected_eras.len() != era_num {
                return false;
            }

            // Ensure eras are all the same for all other nodes.
            nodes
                .values()
                .map(era_ids)
                .all(|eras| eras == expected_eras)
        }
    };

    // Wait for all nodes to agree on one era.
    net.settle_on(&mut rng, is_in_era(1), Duration::from_secs(60))
        .await;

    net.settle_on(&mut rng, is_in_era(2), Duration::from_secs(60))
        .await;
}
