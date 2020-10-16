//! # Casper node client library
#![doc(
    html_root_url = "https://docs.rs/casper-client/0.1.0",
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
mod balance;
mod block;
mod common;
mod error;
mod get_global_state_hash;
mod query_state;
mod rpc;

/// Types and extensions relates to `Deploy`s.
pub mod deploy;

/// Types related to generating keys.
pub mod keygen;

pub use common::ExecutableDeployItemExt;
pub use error::{Error, Result};
pub use rpc::RpcCall;
