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
mod parsing;
mod rpc;
mod validation;

pub mod cl_type;
pub mod keygen;
pub use deploy::ListDeploysResult;
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
/// * `maybe_block_id` must be a hex-encoded, 32-byte hash digest or a `u64` representing the block
///   height or empty.  If empty, the latest block will be used.
pub fn get_state_root_hash(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    maybe_block_id: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.get_state_root_hash(maybe_block_id)
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

/// Gets the bids and validators as of the most recently added block.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `state_root_hash` must be a hex-encoded, 32-byte hash digest.
pub fn get_auction_info(maybe_rpc_id: &str, node_address: &str, verbose: bool) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.get_auction_info()
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

/// Gets a `Block` from the node.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `maybe_block_id` must be a hex-encoded, 32-byte hash digest or a `u64` representing the block
///   height or empty.  If empty, the latest block will be retrieved.
pub fn get_block(
    maybe_rpc_id: &str,
    node_address: &str,
    verbose: bool,
    maybe_block_id: &str,
) -> Result<JsonRpc> {
    RpcCall::new(maybe_rpc_id, node_address, verbose)?.get_block(maybe_block_id)
}

/// Puts a `Deploy` on a node.
///
/// * `maybe_rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned
///   by the node in the corresponding response.  It must be either able to be parsed as a `u32` or
///   empty.  If empty, a random ID will be assigned.
/// * `node_address` identifies the network address of the target node's HTTP server, e.g.
///   `"http://127.0.0.1:7777"`.
/// * When `verbose` is `true`, the request will be printed to `stdout`.
/// * `deploy` contains deploy-related options for this deploy. See `DeployStrParams` for more
///   details.
/// * `session` contains session-related options for this deploy. See `SessionStrParams` for more
///   details.
/// * `payment` contains payment-related options for this deploy. See `PaymentStrParams` for more
///   details.
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
/// * `maybe_output_path` specifies the output file, or if left empty, will log it to `stdout`.
/// * `deploy` contains deploy-related options for this deploy. See `DeployStrParams` for more
///   details.
/// * `session` contains session-related options for this deploy. See `SessionStrParams` for more
///   details.
/// * `payment` contains payment-related options for this deploy. See `PaymentStrParams` for more
///   details.
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
/// * `amount` specifies the amount to be transferred.
/// * `maybe_source_purse` is the source purse in the sender's account that this amount will be
///   drawn from, if it is `""`, it will default to the account's main purse.
/// * `maybe_target_purse` is the target purse in the sender's account that this amount will be
///   transferred to. It is incompatible with `maybe_target_account`.
/// * `maybe_target_account` is the target account that this amount will be transferred to. It is
///   incompatible with `maybe_target_purse`.
/// * `deploy` contains deploy-related options for this deploy. See `DeployStrParams` for more
///   details.
/// * `payment` contains payment-related options for this deploy. See `PaymentStrParams` for more
///   details.
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
    payment: PaymentStrParams<'_>,
) -> Result<JsonRpc> {
    let target = parsing::get_transfer_target(maybe_target_account, maybe_target_purse)?;

    let amount = U512::from_dec_str(amount)
        .map_err(|err| Error::FailedToParseUint(UIntParseError::FromDecStr(err)))?;

    let source_purse = parsing::purse(maybe_source_purse).ok();

    RpcCall::new(maybe_rpc_id, node_address, verbose)?.transfer(
        amount,
        source_purse,
        target,
        deploy.try_into()?,
        payment.try_into()?,
    )
}

/// Signs a `Deploy` file.
///
/// * `input_path` specifies the path to the deploy file.
/// * `secret_key` specifies the key with which to sign the deploy.
/// * `maybe_output_path` specifies the output file, or if left empty, will log it to `stdout`.
pub fn sign_deploy_file(input_path: &str, secret_key: &str, maybe_output_path: &str) -> Result<()> {
    let secret_key = parsing::secret_key(secret_key)?;
    let maybe_output = parsing::output(maybe_output_path);
    Deploy::sign_deploy_file(&input_path, secret_key, maybe_output)
}

/// Container for `Deploy` construction options.
pub struct DeployStrParams<'a> {
    /// Path to secret key file
    pub secret_key: &'a str,
    /// RFC3339-like formatted timestamp. e.g. `2018-02-16T00:31:37Z`. If not provided, the current
    /// time will be used. See https://docs.rs/humantime/latest/humantime/fn.parse_rfc3339_weak.html for more information.
    pub timestamp: &'a str,
    /// Time that the deploy will remain valid for. A deploy can only be included in a block
    /// between `timestamp` and `timestamp + ttl`. Input examples: '1hr 12min', '30min 50sec',
    /// '1day'. For all options, see https://docs.rs/humantime/latest/humantime/fn.parse_duration.html [default: 1hour]
    pub ttl: &'a str,
    /// A hex-encoded deploy hash of a deploy which must be executed before this deploy
    pub dependencies: &'a [&'a str],
    /// Conversion rate between the cost of Wasm opcodes and the motes sent by the payment code
    /// [default: 10]
    pub gas_price: &'a str,
    /// Name of the chain, to avoid the deploy from being accidentally or maliciously included in a
    /// different chain
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
    payment_amount: &'a str,
    payment_hash: &'a str,
    payment_name: &'a str,
    payment_package_hash: &'a str,
    payment_package_name: &'a str,
    payment_path: &'a str,
    payment_args_simple: Vec<&'a str>,
    payment_args_complex: &'a str,
    payment_version: &'a str,
    payment_entry_point: &'a str,
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
            &payment_args_simple,
            payment_args_complex,
            payment_version,
            payment_entry_point,
        )
    }
}

impl<'a> PaymentStrParams<'a> {
    /// Construct PaymentStrParams from a file.
    ///
    /// * `payment_path` is the path to the compiled Wasm payment code.
    /// * See `Self::with_name` for a description of `payment_args_simple` and
    ///   `payment_args_complex`.
    pub fn with_path(
        payment_path: &'a str,
        payment_args_simple: Vec<&'a str>,
        payment_args_complex: &'a str,
    ) -> Self {
        Self {
            payment_path,
            payment_args_simple,
            payment_args_complex,
            ..Default::default()
        }
    }

    /// Construct a PaymentStrParams from an amount.
    ///
    /// `payment_amount` uses the standard-payment system contract rather than custom payment Wasm.
    /// The value is the 'amount' arg of the standard-payment contract.
    pub fn with_amount(payment_amount: &'a str) -> Self {
        Self {
            payment_amount,
            ..Default::default()
        }
    }

    /// Construct PaymentStrParams with a name.
    ///
    /// * `payment_name` is the name of the stored contract (associated with the executing account)
    /// to be called as the payment.
    /// * `payment_entry_point` is the name of the method that will be used when calling the payment
    ///   contract.
    /// * `payment_args_simple` is payment contract arguments, of the form `<NAME:TYPE='VALUE'>...`
    /// For simple CLTypes, a named and typed arg which is passed to the Wasm code. To see an
    /// example for each type, run 'casper-client --show-arg-examples'. This arg can be
    /// repeated to pass multiple named, typed args, but can only be used for the following
    /// types: `bool, i32, i64, u8, u32, u64, u128, u256, u512, unit, string, key, account_hash,
    /// uref, public_key`.
    /// * `payment_args_complex` is the path to file containing named and typed args for passing to
    /// the Wasm code.
    /// Note that only one of `payment_args_simple` and `payment_args_complex` should be used.
    pub fn with_name(
        payment_name: &'a str,
        payment_entry_point: &'a str,
        payment_args_simple: Vec<&'a str>,
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

    /// Construct PaymentStrParams with a hex-encoded hash.
    ///
    /// * `payment_hash` is the hex-encoded hash of the stored contract to be called as the
    /// payment.
    /// * `payment_entry_point` is the name of the method that will be used when calling the payment
    ///   contract.
    /// * See `Self::with_name` for a description of `payment_args_simple` and
    ///   `payment_args_complex`.
    pub fn with_hash(
        payment_hash: &'a str,
        payment_entry_point: &'a str,
        payment_args_simple: Vec<&'a str>,
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

    /// Construct PaymentStrParams with a package name.
    ///
    /// * `payment_package_name` is the name of the stored package to be called as the payment.
    /// * `payment_version` is the version of the called payment contract. Latest will be used by
    ///   default.
    /// * `payment_entry_point` is the name of the method that will be used when calling the payment
    ///   contract.
    /// * See `Self::with_name` for a description of `payment_args_simple` and
    ///   `payment_args_complex`.
    pub fn with_package_name(
        payment_package_name: &'a str,
        payment_version: &'a str,
        payment_entry_point: &'a str,
        payment_args_simple: Vec<&'a str>,
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

    /// Construct PaymentStrParams with a package hash.
    ///
    /// * `payment_package_hash` is the hex-encoded hash of the stored package to be called as the
    ///   payment.
    /// * `payment_version` is the version of the called payment contract. Latest will be used by
    ///   default.
    /// * `payment_entry_point` is the name of the method that will be used when calling the payment
    ///   contract.
    /// * See `Self::with_name` for a description of `payment_args_simple` and
    ///   `payment_args_complex`.
    pub fn with_package_hash(
        payment_package_hash: &'a str,
        payment_version: &'a str,
        payment_entry_point: &'a str,
        payment_args_simple: Vec<&'a str>,
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
            &session_args_simple,
            session_args_complex,
            session_version,
            session_entry_point,
        )
    }
}

/// Container for session related arguments.
#[derive(Default)]
pub struct SessionStrParams<'a> {
    session_hash: &'a str,
    session_name: &'a str,
    session_package_hash: &'a str,
    session_package_name: &'a str,
    session_path: &'a str,
    session_args_simple: Vec<&'a str>,
    session_args_complex: &'a str,
    session_version: &'a str,
    session_entry_point: &'a str,
}

impl<'a> SessionStrParams<'a> {
    /// Construct a SessionStrParams from a file.
    ///
    /// * `session_path` is the path to the compiled Wasm session code.
    /// * See `Self::with_name` for a description of `session_args_simple` and
    ///   `session_args_complex`.
    pub fn with_path(
        session_path: &'a str,
        session_args_simple: Vec<&'a str>,
        session_args_complex: &'a str,
    ) -> Self {
        Self {
            session_path,
            session_args_simple,
            session_args_complex,
            ..Default::default()
        }
    }

    /// Construct `SessionStrParams` with a name.
    ///
    /// * `session_name` is the name of the stored contract (associated with the executing account)
    /// to be called as the session.
    /// * `session_entry_point` is the name of the method that will be used when calling the session
    ///   contract.
    /// * `session_args_simple` is session contract arguments, of the form `<NAME:TYPE='VALUE'>...`
    /// For simple CLTypes, a named and typed arg which is passed to the Wasm code. To see an
    /// example for each type, run 'casper-client --show-arg-examples'. This arg can be
    /// repeated to pass multiple named, typed args, but can only be used for the following
    /// types: `bool, i32, i64, u8, u32, u64, u128, u256, u512, unit, string, key, account_hash,
    /// uref, public_key`.
    /// * `session_args_complex` is the path to file containing named and typed args for passing to
    /// the Wasm code.
    /// Note that only one of `session_args_simple` and `session_args_complex` should be used.
    pub fn with_name(
        session_name: &'a str,
        session_entry_point: &'a str,
        session_args_simple: Vec<&'a str>,
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

    /// Construct `SessionStrParams` with a hex-encoded hash.
    ///
    /// * `session_hash` is the hex-encoded hash of the stored contract to be called as the
    /// session.
    /// * `session_entry_point` is the name of the method that will be used when calling the session
    ///   contract.
    /// * See `Self::with_name` for a description of `session_args_simple` and
    ///   `session_args_complex`.
    pub fn with_hash(
        session_hash: &'a str,
        session_entry_point: &'a str,
        session_args_simple: Vec<&'a str>,
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

    /// Construct `SessionStrParams` with a package name.
    ///
    /// * `session_package_name` is the name of the stored package to be called as the session.
    /// * `session_version` is the version of the called session contract. Latest will be used by
    ///   default.
    /// * `session_entry_point` is the name of the method that will be used when calling the session
    ///   contract.
    /// * See `Self::with_name` for a description of `session_args_simple` and
    ///   `session_args_complex`.
    pub fn with_package_name(
        session_package_name: &'a str,
        session_version: &'a str,
        session_entry_point: &'a str,
        session_args_simple: Vec<&'a str>,
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

    /// Construct SessionStrParams with a package hash.
    ///
    /// * `session_package_hash` is the hex-encoded hash of the stored package to be called as the
    ///   session.
    /// * `session_version` is the version of the called session contract. Latest will be used by
    ///   default.
    /// * `session_entry_point` is the name of the method that will be used when calling the session
    ///   contract.
    /// * See `Self::with_name` for a description of `session_args_simple` and
    ///   `session_args_complex`.
    pub fn with_package_hash(
        session_package_hash: &'a str,
        session_version: &'a str,
        session_entry_point: &'a str,
        session_args_simple: Vec<&'a str>,
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
