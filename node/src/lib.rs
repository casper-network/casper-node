//! # Casper blockchain node
//!
//! This crate contain the core application for the Casper blockchain. Run with `--help` to see
//! available command-line arguments.
//!
//! ## Application structure
//!
//! While the [`main`](fn.main.html) function is the central entrypoint for the node application,
//! its core event loop is found inside the [reactor](reactor/index.html). To get a tour of the
//! sourcecode, be sure to run `cargo doc --open`.

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

pub(crate) use components::small_network;
pub use components::{
    api_server::{rpcs, Config as ApiServerConfig},
    chainspec_loader::{Chainspec, Error as ChainspecError},
    consensus::Config as ConsensusConfig,
    contract_runtime::Config as ContractRuntimeConfig,
    gossiper::{Config as GossipConfig, Error as GossipError},
    small_network::{Config as SmallNetworkConfig, Error as SmallNetworkError},
    storage::{Config as StorageConfig, Error as StorageError},
};
pub use utils::OS_PAGE_SIZE;
