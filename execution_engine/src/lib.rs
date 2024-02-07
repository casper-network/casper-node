//! The engine which executes smart contracts on the Casper network.

#![doc(html_root_url = "https://docs.rs/casper-execution-engine/6.0.0")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon_48.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/blob/dev/images/Casper_Logo_Favicon.png",
    test(attr(deny(warnings)))
)]
#![warn(
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_qualifications
)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod engine_state;
pub mod execution;
pub mod resolvers;
pub mod runtime;
pub mod runtime_context;
