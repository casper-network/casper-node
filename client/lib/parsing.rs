//! This module contains structs and helpers which are used by multiple subcommands related to
//! creating deploys.

use std::{convert::TryInto, fs, path::PathBuf, str::FromStr};

use serde::{self, Deserialize};

use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_node::{
    crypto::{
        asymmetric_key::{PublicKey as NodePublicKey, SecretKey},
        hash::Digest,
    },
    types::{TimeDiff, Timestamp},
};
use casper_types::{
    bytesrepr, CLType, CLValue, ContractHash, Key, NamedArg, RuntimeArgs, UIntParseError, URef,
    U512,
};

use crate::{
    cl_type,
    deploy::DeployParams,
    error::{Error, Result},
    ExecutableDeployItemExt, TransferTarget,
};

pub(super) fn none_if_empty(value: &'_ str) -> Option<&'_ str> {
    if value.is_empty() {
        return None;
    }
    Some(value)
}
/// Handles providing the arg for and retrieval of the timestamp.
mod timestamp {
    use super::*;

    pub(crate) fn parse(value: &str) -> Result<Timestamp> {
        Timestamp::from_str(value).map_err(Error::FailedToParseTimestamp)
    }
}

/// Handles providing the arg for and retrieval of the time to live.
mod ttl {
    use super::*;

    pub(crate) fn parse(value: &str) -> Result<TimeDiff> {
        TimeDiff::from_str(value).map_err(Error::FailedToParseTimeDiff)
    }
}

/// Handles providing the arg for and retrieval of the gas price.
mod gas_price {
    use super::*;

    pub(crate) fn parse(value: &str) -> Result<u64> {
        Ok(value.parse::<u64>()?)
    }
}

/// Handles providing the arg for and retrieval of the deploy dependencies.
mod dependencies {
    use super::*;
    use casper_node::types::DeployHash;

    pub(crate) fn parse(values: &[&str]) -> Result<Vec<DeployHash>> {
        let mut hashes = Vec::with_capacity(values.len());
        for value in values {
            let digest = Digest::from_hex(value)?;
            hashes.push(DeployHash::new(digest))
        }
        Ok(hashes)
    }
}

/// Handles providing the arg for and retrieval of the chain name.
mod chain_name {
    pub(crate) fn parse(value: &str) -> String {
        value.to_string()
    }
}

/// Handles providing the arg for and retrieval of the session code bytes.
mod session_path {
    use super::*;

    pub(crate) fn parse(value: &str) -> Result<Vec<u8>> {
        Ok(fs::read(value)?)
    }
}

/// Handles providing the arg for and retrieval of simple session and payment args.
mod arg_simple {
    use super::*;

    const ARG_VALUE_NAME: &str = "NAME:TYPE='VALUE'";

    pub(crate) mod session {
        use super::*;

        pub fn parse(values: &[&str]) -> Option<RuntimeArgs> {
            if values.is_empty() {
                None
            } else {
                Some(get(values))
            }
        }
    }

    pub(crate) mod payment {
        use super::*;

        pub fn parse(values: &[&str]) -> Option<RuntimeArgs> {
            if values.is_empty() {
                None
            } else {
                Some(get(values))
            }
        }
    }

    fn get(values: &[&str]) -> RuntimeArgs {
        let mut runtime_args = RuntimeArgs::new();
        for arg in values {
            let parts = split_arg(arg);
            parts_to_cl_value(parts, &mut runtime_args);
        }
        runtime_args
    }

    /// Splits a single arg of the form `NAME:TYPE='VALUE'` into its constituent parts.
    fn split_arg(arg: &str) -> (&str, CLType, &str) {
        let parts: Vec<_> = arg.splitn(3, &[':', '='][..]).collect();
        if parts.len() != 3 {
            panic!("arg {} should be formatted as {}", arg, ARG_VALUE_NAME);
        }
        let cl_type = cl_type::parse(&parts[1]).unwrap_or_else(|_| {
            panic!(
                "unknown variant {}, expected one of {}",
                parts[1],
                cl_type::supported_cl_type_list()
            )
        });
        (parts[0], cl_type, parts[2].trim_matches('\''))
    }

    /// Insert a value built from a single arg which has been split into its constituent parts.
    fn parts_to_cl_value(parts: (&str, CLType, &str), runtime_args: &mut RuntimeArgs) {
        let (name, cl_type, value) = parts;
        let cl_value = cl_type::parse_value(cl_type, value)
            .unwrap_or_else(|error| panic!("error parsing cl_value {}", error));
        runtime_args.insert_cl_value(name, cl_value);
    }
}

/// Handles providing the arg for and retrieval of complex session and payment args.  These are read
/// in from a file.
mod args_complex {
    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum DeployArgValue {
        /// Contains `CLValue` serialized into bytes in base16 form.
        #[serde(deserialize_with = "hex::deserialize")]
        RawBytes(Vec<u8>),
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    struct DeployArg {
        /// Deploy argument's name.
        name: String,
        value: DeployArgValue,
    }

    impl From<DeployArgValue> for CLValue {
        fn from(value: DeployArgValue) -> Self {
            match value {
                DeployArgValue::RawBytes(bytes) => bytesrepr::deserialize(bytes)
                    .unwrap_or_else(|error| panic!("should deserialize deploy arg: {}", error)),
            }
        }
    }

    impl From<DeployArg> for NamedArg {
        fn from(deploy_arg: DeployArg) -> Self {
            let cl_value = deploy_arg
                .value
                .try_into()
                .unwrap_or_else(|error| panic!("should serialize deploy arg: {}", error));
            NamedArg::new(deploy_arg.name, cl_value)
        }
    }

    pub mod session {
        use super::*;

        pub fn parse(path: &str) -> Result<RuntimeArgs> {
            if path.is_empty() {
                return Err(Error::InvalidArgument(path.to_string()));
            }
            get(path)
        }
    }

    pub mod payment {
        use super::*;

        pub fn parse(path: &str) -> Result<RuntimeArgs> {
            if path.is_empty() {
                return Err(Error::InvalidArgument(path.to_string()));
            }
            get(path)
        }
    }

    fn get(path: &str) -> Result<RuntimeArgs> {
        let bytes = fs::read(path)?;
        // Received structured args in JSON format.
        let args: Vec<DeployArg> = serde_json::from_slice(&bytes)?;
        // Convert JSON deploy args into vector of named args.
        let mut named_args = Vec::with_capacity(args.len());
        for arg in args {
            named_args.push(arg.into());
        }
        Ok(RuntimeArgs::from(named_args))
    }
}

/// Handles providing the arg for and retrieval of the payment code bytes.
mod payment_path {
    use super::*;

    pub fn parse(path: &str) -> Result<Vec<u8>> {
        Ok(fs::read(path)?)
    }
}

/// Handles providing the arg for and retrieval of the payment-amount arg.
mod standard_payment {

    use super::*;

    const STANDARD_PAYMENT_ARG_NAME: &str = "amount";

    pub fn parse(value: &str) -> Result<RuntimeArgs> {
        let arg = U512::from_dec_str(value)
            .map_err(|err| Error::FailedToParseUint(UIntParseError::FromDecStr(err)))?;
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert(STANDARD_PAYMENT_ARG_NAME, arg);
        Ok(runtime_args)
    }
}

pub(super) mod secret_key {
    use super::*;

    pub fn parse(value: &str) -> Result<SecretKey> {
        let path = PathBuf::from(value);
        SecretKey::from_file(path).map_err(Error::CryptoError)
    }
}

fn args_from_simple_or_complex(
    simple: Option<RuntimeArgs>,
    complex: Option<RuntimeArgs>,
) -> RuntimeArgs {
    // We can have exactly zero or one of the two as `Some`.
    match (simple, complex) {
        (Some(args), None) | (None, Some(args)) => args,
        (None, None) => RuntimeArgs::new(),
        (Some(_), Some(_)) => unreachable!("should not have both simple and complex args"),
    }
}

pub(super) fn parse_deploy_params(
    secret_key: &str,
    timestamp: &str,
    ttl: &str,
    gas_price: &str,
    dependencies: &[&str],
    chain_name: &str,
) -> Result<DeployParams> {
    let secret_key = secret_key::parse(secret_key)?;
    let timestamp = timestamp::parse(timestamp)?;
    let ttl = ttl::parse(ttl)?;
    let gas_price = gas_price::parse(gas_price)?;
    let dependencies = dependencies::parse(dependencies)?;
    let chain_name = chain_name::parse(chain_name);

    Ok(DeployParams {
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        secret_key,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn parse_session_info(
    session_hash: &str,
    session_name: &str,
    package_hash: &str,
    package_name: &str,
    session_path: &str,
    session_args: &[&str],
    session_args_complex: &str,
    version: &str,
    entry_point: &str,
) -> Result<ExecutableDeployItem> {
    let session_args = args_from_simple_or_complex(
        arg_simple::session::parse(session_args),
        args_complex::session::parse(session_args_complex).ok(),
    );

    if let Some(session_name) = session_name::parse(session_name) {
        return ExecutableDeployItem::new_stored_contract_by_name(
            session_name,
            session_entry_point::get(entry_point)
                .ok_or_else(|| Error::InvalidArgument(entry_point.to_string()))?,
            session_args,
        );
    }

    if let Ok(session_hash) = session_hash::parse(session_hash) {
        return ExecutableDeployItem::new_stored_contract_by_hash(
            session_hash,
            session_entry_point::get(entry_point)
                .ok_or_else(|| Error::InvalidArgument(entry_point.to_string()))?,
            session_args,
        );
    }

    let version = session_version::parse(version).ok();
    if let Some(package_name) = session_package_name::get(package_name) {
        return ExecutableDeployItem::new_stored_versioned_contract_by_name(
            package_name,
            version,
            session_entry_point::get(entry_point)
                .ok_or_else(|| Error::InvalidArgument(entry_point.to_string()))?,
            session_args,
        );
    }

    if let Ok(package_hash) = session_package_hash::parse(package_hash) {
        return ExecutableDeployItem::new_stored_versioned_contract_by_hash(
            package_hash,
            version,
            session_entry_point::get(entry_point)
                .ok_or_else(|| Error::InvalidArgument(entry_point.to_string()))?,
            session_args,
        );
    }

    let module_bytes = session_path::parse(session_path)?;
    ExecutableDeployItem::new_module_bytes(module_bytes, session_args)
}

pub(super) fn parse_standard_payment_info(
    standard_payment_amount: &str,
) -> Result<ExecutableDeployItem> {
    let payment_args = standard_payment::parse(standard_payment_amount)?;
    ExecutableDeployItem::new_module_bytes(vec![], payment_args)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn parse_payment_info(
    standard_payment_amount: &str,
    payment_hash: &str,
    payment_name: &str,
    package_hash: &str,
    package_name: &str,
    payment_path: &str,
    payment_args: &[&str],
    payment_args_complex: &str,
    payment_version: &str,
    entry_point: &str,
) -> Result<ExecutableDeployItem> {
    if let Ok(payment_args) = standard_payment::parse(standard_payment_amount) {
        return ExecutableDeployItem::new_module_bytes(vec![], payment_args);
    }

    let payment_args = args_from_simple_or_complex(
        arg_simple::payment::parse(payment_args),
        args_complex::payment::parse(payment_args_complex).ok(),
    );

    if let Some(payment_name) = payment_name::get(payment_name) {
        return ExecutableDeployItem::new_stored_contract_by_name(
            payment_name,
            payment_entry_point::get(entry_point)
                .ok_or_else(|| Error::InvalidArgument(entry_point.to_string()))?,
            payment_args,
        );
    }

    if let Ok(payment_hash) = payment_hash::parse(payment_hash) {
        return ExecutableDeployItem::new_stored_contract_by_hash(
            payment_hash,
            payment_entry_point::get(entry_point)
                .ok_or_else(|| Error::InvalidArgument(entry_point.to_string()))?,
            payment_args,
        );
    }

    let version = payment_version::parse(payment_version).ok();
    if let Some(package_name) = payment_package_name::get(package_name) {
        return ExecutableDeployItem::new_stored_versioned_contract_by_name(
            package_name,
            version,
            payment_entry_point::get(entry_point)
                .ok_or_else(|| Error::InvalidArgument(entry_point.to_string()))?,
            payment_args,
        );
    }

    if let Ok(package_hash) = payment_package_hash::parse(package_hash) {
        return ExecutableDeployItem::new_stored_versioned_contract_by_hash(
            package_hash,
            version,
            payment_entry_point::get(entry_point)
                .ok_or_else(|| Error::InvalidArgument(entry_point.to_string()))?,
            payment_args,
        );
    }

    let module_bytes = payment_path::parse(payment_path)?;
    ExecutableDeployItem::new_module_bytes(module_bytes, payment_args)
}

pub(crate) fn get_transfer_target(
    target_account: &str,
    target_purse: &str,
) -> Result<TransferTarget> {
    let account = target_account::parse(target_account).ok();
    let purse = purse::parse(target_purse).ok();
    let target = match (purse, account) {
        (Some(purse), _) => TransferTarget::OwnPurse(purse),
        (None, Some(account)) => TransferTarget::Account(account),
        _ => {
            return Err(Error::InvalidArgument(format!(
                "Invalid arguments to get_transfer_target {} {}",
                target_purse, target_account
            )))
        }
    };
    Ok(target)
}

pub(super) mod output {
    use super::*;

    pub fn get(value: &str) -> Option<&str> {
        none_if_empty(value)
    }
}

pub(super) mod input {
    pub fn get(value: &str) -> &str {
        value
    }
}

fn parse_contract_hash(value: &str) -> Result<ContractHash> {
    if let Ok(digest) = Digest::from_hex(value) {
        return Ok(digest.to_array());
    }
    if let Ok(Key::Hash(hash)) = Key::from_formatted_str(value) {
        return Ok(hash);
    }
    Err(Error::FailedToParseKey)
}

mod session_hash {
    use super::*;

    pub fn parse(value: &str) -> Result<ContractHash> {
        parse_contract_hash(value)
    }
}

mod session_name {
    use super::*;

    pub fn parse(value: &str) -> Option<String> {
        none_if_empty(value).map(str::to_string)
    }
}

mod session_package_hash {
    use super::*;
    pub fn parse(value: &str) -> Result<ContractHash> {
        parse_contract_hash(value)
    }
}

mod session_package_name {
    use super::*;

    pub fn get(value: &str) -> Option<String> {
        none_if_empty(value).map(str::to_string)
    }
}

mod session_entry_point {
    use super::*;

    pub fn get(value: &str) -> Option<String> {
        none_if_empty(value).map(str::to_string)
    }
}

mod session_version {
    use super::*;

    pub fn parse(value: &str) -> Result<u32> {
        Ok(value.parse::<u32>()?)
    }
}

mod payment_hash {
    use super::*;

    pub fn parse(value: &str) -> Result<ContractHash> {
        parse_contract_hash(value)
    }
}

mod payment_name {
    use super::*;

    pub fn get(value: &str) -> Option<String> {
        none_if_empty(value).map(str::to_string)
    }
}

mod payment_package_hash {
    use super::*;

    pub fn parse(value: &str) -> Result<ContractHash> {
        parse_contract_hash(value)
    }
}

mod payment_package_name {
    use super::*;

    pub fn get(value: &str) -> Option<String> {
        none_if_empty(value).map(str::to_string)
    }
}

mod payment_entry_point {
    use super::*;

    pub fn get(value: &str) -> Option<String> {
        none_if_empty(value).map(str::to_string)
    }
}

mod payment_version {
    use super::*;

    pub fn parse(value: &str) -> Result<u32> {
        Ok(value.parse::<u32>()?)
    }
}

mod target_account {

    use super::*;

    pub(crate) fn parse(value: &str) -> Result<NodePublicKey> {
        Ok(NodePublicKey::from_hex(value)?)
    }
}

pub(super) mod purse {

    use super::*;

    pub(crate) fn parse(value: &str) -> Result<URef> {
        Ok(URef::from_formatted_str(value)?)
    }
}
