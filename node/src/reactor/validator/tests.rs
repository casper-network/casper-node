// FIXME: Remove
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unreachable_code)]

use rand::Rng;
use tempfile::TempDir;

use crate::{
    components::{contract_runtime::core::engine_state::genesis::GenesisConfig, storage},
    crypto::asymmetric_key::SecretKey,
    reactor::{initializer, validator, Runner},
    testing::{init_logging, network::Network, TestRng},
    types::{Motes, NodeConfig},
    utils::{External, Loadable, WithDir, RESOURCES_PATH},
    Chainspec, GenesisAccount,
};
use anyhow::bail;
use casperlabs_types::U512;
use std::time::Duration;

struct TestChain {
    keys: Vec<SecretKey>,
    storages: Vec<TempDir>,
    chainspec: Chainspec,
}

impl TestChain {
    /// Instances a new test chain configuration.
    ///
    /// Generates private keys for `size` validators and creates a matching chainspec.
    fn new<R: Rng + ?Sized>(rng: &mut R, size: usize) -> Self {
        // Create a secret key for each validator.
        let keys: Vec<SecretKey> = (0..size).map(|_| rng.gen()).collect();

        // Load the `local` chainspec.
        let mut chainspec = Chainspec::bundled("local/chainspec.toml");

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

    net.settle_on(&mut rng, |_| false, Duration::from_secs(500))
        .await;
}
