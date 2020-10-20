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
pub mod cl_type;
mod deploy;
mod error;
mod executable_deploy_item_ext;
mod get_state_hash;
pub mod keygen;
mod query_state;
mod rpc;

pub use deploy::{DeployExt, DeployParams};
pub use error::Error;
pub use executable_deploy_item_ext::ExecutableDeployItemExt;
pub use rpc::RpcCall;
