//! # Casper blockchain node
//!
//! This crate contain the core application for the Casper blockchain. Run with `--help` to see
//! available command-line arguments.
//!
//! ## Application structure
//!
//! While the [`main`](fn.main.html) function is the central entrypoint for the node application,
//! its core event loop is found inside the [reactor](reactor/index.html).

#![doc(html_root_url = "https://docs.rs/casper-node/1.7.0")]
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
#![feature(test)]

extern crate test;

pub mod components;
pub mod crypto;
pub mod effect;
pub mod logging;
pub mod protocol;
pub mod reactor;
#[cfg(test)]
pub mod testing;
pub mod tls;
pub mod types;
pub mod utils;

use ansi_term::Color::Red;
use lazy_static::lazy_static;

pub(crate) use components::small_network;
pub use components::{
    api_server::{rpcs, Config as ApiServerConfig},
    chainspec_loader::{Chainspec, Error as ChainspecError},
    consensus::Config as ConsensusConfig,
    contract_runtime::Config as ContractRuntimeConfig,
    fetcher::Config as FetcherConfig,
    gossiper::{Config as GossipConfig, Error as GossipError},
    small_network::{Config as SmallNetworkConfig, Error as SmallNetworkError},
    storage::{Config as StorageConfig, Error as StorageError},
};
pub use utils::OS_PAGE_SIZE;

/// The maximum thread count which should be spawned by the tokio runtime.
pub const MAX_THREAD_COUNT: usize = 512;

fn version_string(color: bool) -> String {
    let mut version = if env!("VERGEN_SEMVER_LIGHTWEIGHT") == "UNKNOWN" {
        env!("CARGO_PKG_VERSION").to_string()
    } else {
        format!(
            "{}-{}",
            env!("VERGEN_SEMVER_LIGHTWEIGHT"),
            env!("VERGEN_SHA_SHORT"),
        )
    };

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

lazy_static! {
    /// Color version string for the compiled node. Filled in at build time, output allocated at
    /// runtime.
    pub static ref VERSION_STRING_COLOR: String = version_string(true);

    /// Version string for the compiled node. Filled in at build time, output allocated at runtime.
    pub static ref VERSION_STRING: String = version_string(false);
}
