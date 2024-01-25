//! # Casper blockchain node
//!
//! This crate contain the core application for the Casper blockchain. Run with `--help` to see
//! available command-line arguments.
//!
//! ## Application structure
//!
//! While the [`main`](fn.main.html) function is the central entrypoint for the node application,
//! its core event loop is found inside the [reactor](reactor/index.html).

#![doc(html_root_url = "https://docs.rs/casper-node/1.5.6")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon_48.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon.png",
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
#[cfg_attr(not(feature = "failpoints"), path = "failpoints_disabled.rs")]
pub(crate) mod failpoints;

pub mod logging;
pub(crate) mod protocol;
pub(crate) mod reactor;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod tls;
pub mod types;
pub mod utils;

use std::{
    env,
    sync::{atomic::AtomicUsize, Arc},
};

use ansi_term::Color::Red;
use once_cell::sync::Lazy;
#[cfg(not(test))]
use rand::SeedableRng;
use signal_hook::{consts::TERM_SIGNALS, flag};
use tracing::warn;

pub(crate) use components::{
    block_accumulator::Config as BlockAccumulatorConfig,
    block_synchronizer::Config as BlockSynchronizerConfig,
    block_validator::Config as BlockValidatorConfig,
    consensus::Config as ConsensusConfig,
    contract_runtime::Config as ContractRuntimeConfig,
    deploy_acceptor::Config as DeployAcceptorConfig,
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
pub use components::{
    consensus, contract_runtime,
    rpc_server::rpcs,
    storage::{self, Config as StorageConfig},
};
pub use reactor::main_reactor::Config as MainReactorConfig;
pub(crate) use types::NodeRng;
pub use utils::WithDir;

/// The maximum thread count which should be spawned by the tokio runtime.
pub const MAX_THREAD_COUNT: usize = 512;

fn version_string(color: bool) -> String {
    let mut version = env!("CARGO_PKG_VERSION").to_string();
    if let Ok(git_sha) = env::var("VERGEN_GIT_SHA") {
        version = format!("{}-{}", version, git_sha);
    } else {
        warn!(
            "vergen env var unavailable, casper-node build version will not include git short hash"
        );
    }

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
