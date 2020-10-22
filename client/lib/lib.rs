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

pub mod cl_type;
mod deploy;
mod error;
mod executable_deploy_item_ext;
pub mod keygen;
mod rpc;

use jsonrpc_lite::JsonRpc;

pub use deploy::{DeployExt, DeployParams};
pub use error::Error;
use error::Result;
pub use executable_deploy_item_ext::ExecutableDeployItemExt;
pub use rpc::{RpcCall, TransferTarget};

/// Gets a `Deploy` from the node.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `deploy_hash` must be a hex-encoded, 32-byte hash digest.
pub fn get_deploy(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    deploy_hash: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.get_deploy(deploy_hash)
}

/// Queries the node for an item at the given state root hash, under the given key and path.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `state_root_hash` must be a hex-encoded, 32-byte hash digest.
/// * `key` must be a formatted [`PublicKey`](casper_node::crypto::asymmetric_key::PublicKey) or
///   [`Key`](casper_types::Key).  This will take the form of e.g.
///       * `01c9e33693951aaac23c49bee44ad6f863eedcd38c084a3a8f11237716a3df9c2c` or
///       * `account-hash-0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20` or
///       * `hash-0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20` or
///       * `uref-0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20-007`
/// * `path` is comprised of components starting from the `key`, separated by `/`s.
pub fn get_item(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    state_root_hash: &str,
    key: &str,
    path: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.get_item(state_root_hash, key, path)
}

/// Queries the node for a state root hash at a given `Block`.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `maybe_block_hash` must be a hex-encoded, 32-byte hash digest or empty.  If empty, the latest
///   block will be used.
pub fn get_state_root_hash(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    maybe_block_hash: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.get_state_root_hash(maybe_block_hash)
}

/// Gets the balance from a purse.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `state_root_hash` must be a hex-encoded, 32-byte hash digest.
/// * `purse_uref` must be a formatted URef as obtained with `get_item`.  This will take the form of
///   e.g. `uref-0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20-007`
pub fn get_balance(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    state_root_hash: &str,
    purse_uref: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.get_balance(state_root_hash, purse_uref)
}

/// Reads a previously-saved `Deploy` from file, and sends that to the node.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `input_path` is the path to the file to read.
pub fn send_deploy_file(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    input_path: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.send_deploy_file(input_path)
}

/// Lists `Deploy`s included in the specified `Block`.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `maybe_block_hash` must be a hex-encoded, 32-byte hash digest or empty.  If empty, the latest
///   block will be used.
pub fn list_deploys(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    maybe_block_hash: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.list_deploys(maybe_block_hash)
}

/// Gets a `Block` from the node.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `maybe_block_hash` must be a hex-encoded, 32-byte hash digest or empty.  If empty, the latest
///   block will be retrieved.
pub fn get_block(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    maybe_block_hash: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.get_block(maybe_block_hash)
}
