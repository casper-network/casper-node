//! # Casper blockchain node
//!
//! This crate contain the core application for the Casper blockchain. Run with `--help` to see
//! available command-line arguments.
//!
//! ## Application structure
//!
//! While the [`main`](fn.main.html) function is the central entrypoint for the node application,
//! its core event loop is found inside the [reactor](reactor/index.html).

#![doc(html_root_url = "https://docs.rs/casper-node/1.4.8")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_qualifications
)]
#![allow(clippy::bool_comparison)]

pub mod cli;
pub(crate) mod components;
mod config_migration;
mod data_migration;
pub(crate) mod effect;
pub mod logging;
pub(crate) mod protocol;
pub(crate) mod reactor;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod tls;
pub mod types;
pub mod utils;
pub use components::{
    contract_runtime,
    rpc_server::rpcs,
    storage::{self, Config as StorageConfig},
};
pub use reactor::main_reactor::Config as MainReactorConfig;
pub use utils::WithDir;

use std::sync::{atomic::AtomicUsize, Arc};

use ansi_term::Color::Red;
use once_cell::sync::Lazy;
#[cfg(not(test))]
use rand::SeedableRng;
use signal_hook::{consts::TERM_SIGNALS, flag};

pub(crate) use components::{
    block_accumulator::Config as BlockAccumulatorConfig,
    block_synchronizer::Config as BlockSynchronizerConfig,
    consensus::Config as ConsensusConfig,
    contract_runtime::Config as ContractRuntimeConfig,
    deploy_buffer::Config as DeployBufferConfig,
    diagnostics_port::Config as DiagnosticsPortConfig,
    event_stream_server::Config as EventStreamServerConfig,
    fetcher::Config as FetcherConfig,
    gossiper::Config as GossipConfig,
    network::Config as NetworkConfig,
    rest_server::Config as RestServerConfig,
    rpc_server::{Config as RpcServerConfig, SpeculativeExecConfig},
    upgrade_watcher::Config as UpgradeWatcherConfig,
};
pub(crate) use types::NodeRng;

/// The maximum thread count which should be spawned by the tokio runtime.
pub const MAX_THREAD_COUNT: usize = 512;

fn version_string(color: bool) -> String {
    let mut version = format!(
        "{}-{}",
        env!("CARGO_PKG_VERSION"),
        env!("VERGEN_GIT_SHA_SHORT")
    );

    // Add a `@DEBUG` (or similar) tag to release string on non-release builds.
    if env!("NODE_BUILD_PROFILE") != "release" {
        version += "@";
        let profile = env!("NODE_BUILD_PROFILE").to_uppercase();
        version.push_str(&if color {
            Red.paint(&profile).to_string()
        } else {
            profile
        });
    }

    version
}

/// Color version string for the compiled node. Filled in at build time, output allocated at
/// runtime.
pub(crate) static VERSION_STRING_COLOR: Lazy<String> = Lazy::new(|| version_string(true));

/// Version string for the compiled node. Filled in at build time, output allocated at runtime.
pub(crate) static VERSION_STRING: Lazy<String> = Lazy::new(|| version_string(false));

/// Global value that indicates the currently running reactor should exit if it is non-zero.
pub(crate) static TERMINATION_REQUESTED: Lazy<Arc<AtomicUsize>> =
    Lazy::new(|| Arc::new(AtomicUsize::new(0)));

/// Setup UNIX signal hooks for current application.
pub(crate) fn setup_signal_hooks() {
    for signal in TERM_SIGNALS {
        flag::register_usize(
            *signal,
            Arc::clone(&*TERMINATION_REQUESTED),
            *signal as usize,
        )
        .unwrap_or_else(|error| panic!("failed to register signal {}: {}", signal, error));
    }
}

/// Constructs a new `NodeRng`.
#[cfg(not(test))]
pub(crate) fn new_rng() -> NodeRng {
    NodeRng::from_entropy()
}

/// Constructs a new `NodeRng`.
#[cfg(test)]
pub(crate) fn new_rng() -> NodeRng {
    NodeRng::new()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, iter};

    use casper_types::{crypto, testing::TestRng, PublicKey, SecretKey, Timestamp, U512};

    use crate::types::{Block, FinalizedBlock};

    pub(crate) struct ValidatorSpec {
        pub(crate) secret_key: SecretKey,
        pub(crate) public_key: PublicKey,
        // If `None`, weight will be chosen randomly.
        pub(crate) weight: Option<U512>,
    }

    // Utility struct that can be turned into an iterator that generates
    // continuous and descending blocks (i.e. blocks that have consecutive height
    // and parent hashes are correctly set). The height of the first block
    // in a series is chosen randomly.
    //
    // Additionally, this struct allows to generate switch blocks at a specific location in the
    // chain, for example: Setting `switch_block_indices` to [1; 3] and generating 5 blocks will
    // cause the 2nd and 4th blocks to be switch blocks. Validators for all eras are filled from
    // the `validators` parameter using the weight, if specified.
    pub(crate) struct TestChainSpec<'a> {
        block: Block,
        rng: &'a mut TestRng,
        switch_block_indices: Option<Vec<u64>>,
        validators: &'a [ValidatorSpec],
    }

    impl<'a> TestChainSpec<'a> {
        pub(crate) fn new(
            test_rng: &'a mut TestRng,
            switch_block_indices: Option<Vec<u64>>,
            validators: &'a [ValidatorSpec],
        ) -> Self {
            let block = Block::random(test_rng);
            Self {
                block,
                rng: test_rng,
                switch_block_indices,
                validators,
            }
        }

        pub(crate) fn iter(&mut self) -> TestBlockIterator {
            let block_height = self.block.height();

            const DEFAULT_VALIDATOR_WEIGHT: u64 = 100;

            TestBlockIterator::new(
                self.block.clone(),
                self.rng,
                self.switch_block_indices
                    .clone()
                    .map(|switch_block_indices| {
                        switch_block_indices
                            .iter()
                            .map(|index| index + block_height)
                            .collect()
                    }),
                self.validators
                    .iter()
                    .map(
                        |ValidatorSpec {
                             secret_key: _,
                             public_key,
                             weight,
                         }| {
                            (
                                public_key.clone(),
                                weight.unwrap_or(DEFAULT_VALIDATOR_WEIGHT.into()),
                            )
                        },
                    )
                    .collect(),
            )
        }
    }

    pub(crate) struct TestBlockIterator<'a> {
        block: Block,
        rng: &'a mut TestRng,
        switch_block_indices: Option<Vec<u64>>,
        validators: Vec<(PublicKey, U512)>,
        next_validator_index: usize,
    }

    impl<'a> TestBlockIterator<'a> {
        pub fn new(
            block: Block,
            rng: &'a mut TestRng,
            switch_block_indices: Option<Vec<u64>>,
            validators: Vec<(PublicKey, U512)>,
        ) -> Self {
            Self {
                block,
                rng,
                switch_block_indices,
                validators,
                next_validator_index: 0,
            }
        }
    }

    impl<'a> Iterator for TestBlockIterator<'a> {
        type Item = Block;

        fn next(&mut self) -> Option<Self::Item> {
            let (is_switch_block, is_successor_of_switch_block, validators) =
                match &self.switch_block_indices {
                    Some(switch_block_heights)
                        if switch_block_heights.contains(&self.block.height()) =>
                    {
                        let is_successor_of_switch_block =
                            switch_block_heights.contains(&(self.block.height().saturating_sub(1)));
                        (
                            true,
                            is_successor_of_switch_block,
                            Some(self.validators.clone()),
                        )
                    }
                    Some(switch_block_heights) => {
                        let is_successor_of_switch_block =
                            switch_block_heights.contains(&(self.block.height().saturating_sub(1)));
                        (false, is_successor_of_switch_block, None)
                    }
                    None => (false, false, None),
                };

            let validators = if let Some(validators) = validators {
                let first_validator = validators.get(self.next_validator_index).unwrap();
                let second_validator = validators.get(self.next_validator_index + 1).unwrap();

                // Put two validators in each switch block.
                let mut validators_for_block = BTreeMap::new();
                validators_for_block.insert(first_validator.0.clone(), first_validator.1);
                validators_for_block.insert(second_validator.0.clone(), second_validator.1);
                self.next_validator_index += 2;

                // If we're out of validators, do round robin on the provided list.
                if self.next_validator_index >= self.validators.len() {
                    self.next_validator_index = 0;
                }
                Some(validators_for_block)
            } else {
                None
            };

            let next = Block::new(
                *self.block.hash(),
                self.block.header().accumulated_seed(),
                *self.block.header().state_root_hash(),
                FinalizedBlock::random_with_specifics(
                    self.rng,
                    if is_successor_of_switch_block {
                        self.block.header().era_id().successor()
                    } else {
                        self.block.header().era_id()
                    },
                    self.block.header().height() + 1,
                    is_switch_block,
                    Timestamp::now(),
                    iter::empty(),
                ),
                validators,
                self.block.header().protocol_version(),
            )
            .unwrap();
            self.block = next.clone();
            Some(next)
        }
    }

    #[test]
    fn test_block_iter() {
        let mut rng = TestRng::new();
        let mut test_block = TestChainSpec::new(&mut rng, None, &[]);
        let mut block_batch = test_block.iter().take(100);
        let mut parent_block: Block = block_batch.next().unwrap();
        for current_block in block_batch {
            assert_eq!(
                current_block.header().height(),
                parent_block.header().height() + 1,
                "height should grow monotonically"
            );
            assert_eq!(
                current_block.header().parent_hash(),
                parent_block.hash(),
                "block's parent should point at previous block"
            );
            parent_block = current_block;
        }
    }

    #[test]
    fn test_block_iter_creates_switch_blocks() {
        let switch_block_indices = vec![0, 10, 76];

        let validators: Vec<_> = iter::repeat_with(crypto::generate_ed25519_keypair)
            .take(2)
            .map(|(secret_key, public_key)| ValidatorSpec {
                secret_key,
                public_key,
                weight: None,
            })
            .collect();

        let mut rng = TestRng::new();
        let mut test_block =
            TestChainSpec::new(&mut rng, Some(switch_block_indices.clone()), &validators);
        let block_batch: Vec<_> = test_block.iter().take(100).collect();

        let base_height = block_batch.first().expect("should have block").height();

        for block in block_batch {
            if switch_block_indices
                .iter()
                .map(|index| index + base_height)
                .any(|index| index == block.height())
            {
                assert!(block.header().is_switch_block())
            } else {
                assert!(!block.header().is_switch_block())
            }
        }
    }
}
