//! The engine which executes smart contracts on the Casper network.

#![doc(html_root_url = "https://docs.rs/casper-execution-engine/7.0.1")]
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

pub mod config;
pub mod core;
pub mod shared;
/// Storage for the execution engine.
pub mod storage;
mod system;
