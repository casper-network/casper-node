use std::{collections::HashSet, time::Duration};

use anyhow::bail;
use rand::Rng;
use tempfile::TempDir;

use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use casper_types::U512;

use crate::{
    components::{consensus::EraId, small_network, storage},
    crypto::asymmetric_key::{PublicKey, SecretKey},
    reactor::{initializer, joiner, validator, Runner},
    testing::{self, network::Network, ConditionCheckReactor, TestRng},
    utils::{External, Loadable, WithDir, RESOURCES_PATH},
    Chainspec,
};

struct TestChain {
    keys: Vec<SecretKey>,
    storages: Vec<TempDir>,
    chainspec: Chainspec,
}

type Nodes = crate::testing::network::Nodes<validator::Reactor<TestRng>>;

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
                let public_key: PublicKey = secret_key.into();
                GenesisAccount::new(
                    public_key.into(),
                    public_key.to_account_hash(),
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
    fn create_node_config(&mut self, idx: usize, first_node_port: u16) -> validator::Config {
        // Start with a default configuration.
        let mut cfg = validator::Config::default();

        // Set the network configuration.
        cfg.network = if idx == 0 {
            small_network::Config::default_local_net_first_node(first_node_port)
        } else {
            small_network::Config::default_local_net(first_node_port)
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
        rng: &mut TestRng,
    ) -> anyhow::Result<Network<validator::Reactor<TestRng>>> {
        let root = RESOURCES_PATH.join("local");

        let mut network: Network<validator::Reactor<TestRng>> = Network::new();
        let first_node_port = testing::unused_port_on_localhost();

        for idx in 0..self.keys.len() {
            let cfg = self.create_node_config(idx, first_node_port);

            // We create an initializer reactor here and run it to completion.
            let mut initializer_runner =
                Runner::<initializer::Reactor, TestRng>::new(WithDir::new(root.clone(), cfg), rng)
                    .await?;
            initializer_runner.run(rng).await;

            // Now we can construct the actual node.
            let initializer = initializer_runner.into_inner();
            if !initializer.stopped_successfully() {
                bail!("failed to initialize successfully");
            }

            let mut joiner_runner = Runner::<joiner::Reactor, TestRng>::new(
                WithDir::new(root.clone(), initializer),
                rng,
            )
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
fn era_ids(
    runner: &Runner<ConditionCheckReactor<validator::Reactor<TestRng>>, TestRng>,
) -> HashSet<EraId> {
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
    testing::init_logging();

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
            if expected_eras.len() <= era_num {
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
