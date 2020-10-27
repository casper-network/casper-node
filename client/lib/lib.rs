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

mod deploy;
mod error;
mod executable_deploy_item_ext;
mod merkle_proofs;
mod parsing;
mod rpc;

pub mod cl_type;
pub mod keygen;
pub use error::Error;

use std::convert::TryInto;

use jsonrpc_lite::JsonRpc;

use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_node::types::Deploy;
use casper_types::{UIntParseError, U512};

use deploy::{DeployExt, DeployParams};
use error::Result;
use executable_deploy_item_ext::ExecutableDeployItemExt;
use parsing::none_if_empty;
use rpc::{RpcCall, TransferTarget};

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

/// Puts a `Deploy` on a node.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `deploy` - Deploy arguments container struct holding options for this deploy. See
///   `DeployStrParams`.
/// * `session` - Session arguments container struct holding options for this deploy. See
///   `SessionStrParams`.
/// * `payment` - Payment arguments container struct holding options for this deploy. See
///   `PaymentStrParams`.
pub fn put_deploy(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    deploy: DeployStrParams<'_>,
    session: SessionStrParams<'_>,
    payment: PaymentStrParams<'_>,
) -> Result<JsonRpc> {
    let deploy = Deploy::with_payment_and_session(
        deploy.try_into()?,
        payment.try_into()?,
        session.try_into()?,
    );
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.put_deploy(deploy)
}

/// Make a deploy and output to file or stdout.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `deploy` - Deploy arguments container struct holding options for this deploy. See
///   `DeployStrParams`.
/// * `session` - Session arguments container struct holding options for this deploy. See
///   `SessionStrParams`.
/// * `payment` - Payment arguments container struct holding options for this deploy. See
///   `PaymentStrParams`.
#[allow(clippy::too_many_arguments)]
pub fn make_deploy(
    maybe_output_path: &str,
    deploy: DeployStrParams<'_>,
    session: SessionStrParams<'_>,
    payment: PaymentStrParams<'_>,
) -> Result<()> {
    Deploy::make_deploy(
        none_if_empty(maybe_output_path),
        deploy.try_into()?,
        payment.try_into()?,
        session.try_into()?,
    )
}

/// Transfers an amount between purses.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `amount` - Specifies the amount to be transferred.
/// * `maybe_source_purse` - Source purse in the sender's account that this amount will be drawn
///   from, if it is `""`, it will default to the account's main purse.
/// * `maybe_target_purse` - Target purse in the sender's account that this amount will be
///   transferred to.
/// it is incompatible with `maybe_target_account`.
/// * `maybe_target_account` - Target account that this amount will be transferred to.
/// it is incompatible with `maybe_target_purse`.
/// * `deploy` - Deploy arguments container struct holding options for this deploy.
#[allow(clippy::too_many_arguments)]
pub fn transfer(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    amount: &str,
    maybe_source_purse: &str,
    maybe_target_purse: &str,
    maybe_target_account: &str,
    deploy: DeployStrParams<'_>,
) -> Result<JsonRpc> {
    let target = parsing::get_transfer_target(maybe_target_account, maybe_target_purse)?;

    // transfer always invoke the standard payment
    let payment = parsing::parse_standard_payment_info(amount)?;
    let amount = U512::from_dec_str(amount)
        .map_err(|err| Error::FailedToParseUint(UIntParseError::FromDecStr(err)))?;

    let source_purse = parsing::purse::parse(maybe_source_purse).ok();

    RpcCall::new(maybe_rpc_id, node_address, verbose)?.transfer(
        amount,
        source_purse,
        target,
        deploy.try_into()?,
        payment,
    )
}

/// Signs a `Deploy` file.
///
/// * `input_path` specifies the path to the deploy file.
/// * `secret_key` specifies the key with which to sign the deploy.
/// * `maybe_output` specifies the output file, or if left empty, will log it to `stdout`.
pub fn sign_deploy_file(input: &str, secret_key: &str, maybe_output: &str) -> Result<()> {
    let input_path = parsing::input::get(input);
    let secret_key = parsing::secret_key::parse(secret_key)?;
    let maybe_output = parsing::output::get(maybe_output);
    Deploy::sign_deploy_file(&input_path, secret_key, maybe_output)
}

/// Container for `Deploy` construction options.
pub struct DeployStrParams<'a> {
    /// Formatted string of `SecretKey`.
    pub secret_key: &'a str,
    /// Formatted string of `Timestamp`.
    pub timestamp: &'a str,
    /// Formatted string of `ttl`.
    pub ttl: &'a str,
    /// List of formatted `DeployHash` strings.
    pub dependencies: &'a [&'a str],
    /// Gas price (uint) as a string.
    pub gas_price: &'a str,
    /// Chain name.
    pub chain_name: &'a str,
}

impl<'a> TryInto<DeployParams> for DeployStrParams<'a> {
    type Error = Error;

    fn try_into(self) -> Result<DeployParams> {
        let DeployStrParams {
            secret_key,
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
        } = self;
        parsing::parse_deploy_params(
            secret_key,
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
        )
    }
}

/// Container for payment related arguments.
#[derive(Default)]
pub struct PaymentStrParams<'a> {
    /// Payment amount - all other `payment_*` arguments are ignored and the standard payment
    /// contract is used if this is set.
    pub payment_amount: &'a str,
    /// Hash of this payment.
    pub payment_hash: &'a str,
    /// Name for payment.
    pub payment_name: &'a str,
    /// Hash of payment package to be used.
    pub payment_package_hash: &'a str,
    /// Name to be used for payment package.
    pub payment_package_name: &'a str,
    /// Path to file containing payment code bytes.
    pub payment_path: &'a str,
    ///Inline args of the form `NAME:TYPE=VALUE`. Incompatible with
    ///   `payment_args_complex`.
    pub payment_args_simple: &'a [&'a str],
    ///Complex args in file form. Incompatible with `payment_args_simple`.
    pub payment_args_complex: &'a str,
    /// Version of this payment.
    pub payment_version: &'a str,
    /// Entry point for payment.
    pub payment_entry_point: &'a str,
}

impl<'a> TryInto<ExecutableDeployItem> for PaymentStrParams<'a> {
    type Error = Error;

    fn try_into(self) -> Result<ExecutableDeployItem> {
        let PaymentStrParams {
            payment_amount,
            payment_hash,
            payment_name,
            payment_package_hash,
            payment_package_name,
            payment_path,
            payment_args_simple,
            payment_args_complex,
            payment_version,
            payment_entry_point,
        } = self;

        parsing::parse_payment_info(
            payment_amount,
            payment_hash,
            payment_name,
            payment_package_hash,
            payment_package_name,
            payment_path,
            payment_args_simple,
            payment_args_complex,
            payment_version,
            payment_entry_point,
        )
    }
}

impl<'a> PaymentStrParams<'a> {
    /// Construct PaymentParams from a file.
    pub fn with_path(payment_path: &'a str) -> Self {
        Self {
            payment_path,
            ..Default::default()
        }
    }

    /// Construct PaymentParams using an amount.
    pub fn with_amount(payment_amount: &'a str) -> Self {
        Self {
            payment_amount,
            ..Default::default()
        }
    }

    /// Construct PaymentParams with a name.
    pub fn with_name(
        payment_name: &'a str,
        payment_entry_point: &'a str,
        payment_args_simple: &'a [&'a str],
        payment_args_complex: &'a str,
    ) -> Self {
        Self {
            payment_name,
            payment_entry_point,
            payment_args_simple,
            payment_args_complex,
            ..Default::default()
        }
    }

    /// Construct PaymentParams with a hash.
    pub fn with_hash(
        payment_hash: &'a str,
        payment_entry_point: &'a str,
        payment_args_simple: &'a [&'a str],
        payment_args_complex: &'a str,
    ) -> Self {
        Self {
            payment_hash,
            payment_entry_point,
            payment_args_simple,
            payment_args_complex,
            ..Default::default()
        }
    }

    /// Construct PaymentParams with a package name.
    pub fn with_package_name(
        payment_package_name: &'a str,
        payment_version: &'a str,
        payment_entry_point: &'a str,
        payment_args_simple: &'a [&'a str],
        payment_args_complex: &'a str,
    ) -> Self {
        Self {
            payment_package_name,
            payment_version,
            payment_entry_point,
            payment_args_simple,
            payment_args_complex,
            ..Default::default()
        }
    }

    /// Construct PaymentParams with a package hash.
    pub fn with_package_hash(
        payment_package_hash: &'a str,
        payment_version: &'a str,
        payment_entry_point: &'a str,
        payment_args_simple: &'a [&'a str],
        payment_args_complex: &'a str,
    ) -> Self {
        Self {
            payment_package_hash,
            payment_version,
            payment_entry_point,
            payment_args_simple,
            payment_args_complex,
            ..Default::default()
        }
    }
}

impl<'a> TryInto<ExecutableDeployItem> for SessionStrParams<'a> {
    type Error = Error;

    fn try_into(self) -> Result<ExecutableDeployItem> {
        let SessionStrParams {
            session_hash,
            session_name,
            session_package_hash,
            session_package_name,
            session_path,
            session_args_simple,
            session_args_complex,
            session_version,
            session_entry_point,
        } = self;

        parsing::parse_session_info(
            session_hash,
            session_name,
            session_package_hash,
            session_package_name,
            session_path,
            session_args_simple,
            session_args_complex,
            session_version,
            session_entry_point,
        )
    }
}

/// Container for session related arguments.
#[derive(Default)]
pub struct SessionStrParams<'a> {
    /// Hash for this session.
    pub session_hash: &'a str,
    /// Name for this session.
    pub session_name: &'a str,
    /// Package hash for this session.
    pub session_package_hash: &'a str,
    /// Package Name for this session.
    pub session_package_name: &'a str,
    /// Path to file containing session code bytes.
    pub session_path: &'a str,
    /// Inline args of the form `NAME:TYPE=VALUE`. Incompatible with
    ///   `session_args_complex`.
    pub session_args_simple: &'a [&'a str],
    /// Complex args in file form. Incompatible with `session_args_simple`.
    pub session_args_complex: &'a str,
    /// Version for this session.
    pub session_version: &'a str,
    /// Entry point for this session.
    pub session_entry_point: &'a str,
}

impl<'a> SessionStrParams<'a> {
    /// Construct a SessionParams from a file.
    pub fn with_path(session_path: &'a str) -> Self {
        Self {
            session_path,
            ..Default::default()
        }
    }

    /// Construct SessionParams with a name.
    pub fn with_name(
        session_name: &'a str,
        session_entry_point: &'a str,
        session_args_simple: &'a [&'a str],
        session_args_complex: &'a str,
    ) -> Self {
        Self {
            session_name,
            session_entry_point,
            session_args_simple,
            session_args_complex,
            ..Default::default()
        }
    }

    /// Construct SessionParams with a hash.
    pub fn with_hash(
        session_hash: &'a str,
        session_entry_point: &'a str,
        session_args_simple: &'a [&'a str],
        session_args_complex: &'a str,
    ) -> Self {
        Self {
            session_hash,
            session_entry_point,
            session_args_simple,
            session_args_complex,
            ..Default::default()
        }
    }

    /// Construct SessionParams with a package name.
    pub fn with_package_name(
        session_package_name: &'a str,
        session_version: &'a str,
        session_entry_point: &'a str,
        session_args_simple: &'a [&'a str],
        session_args_complex: &'a str,
    ) -> Self {
        Self {
            session_package_name,
            session_version,
            session_entry_point,
            session_args_simple,
            session_args_complex,
            ..Default::default()
        }
    }

    /// Construct SessionParams with a package hash.
    pub fn with_package_hash(
        session_package_hash: &'a str,
        session_version: &'a str,
        session_entry_point: &'a str,
        session_args_simple: &'a [&'a str],
        session_args_complex: &'a str,
    ) -> Self {
        Self {
            session_package_hash,
            session_version,
            session_entry_point,
            session_args_simple,
            session_args_complex,
            ..Default::default()
        }
    }
}
