//! This module contains structs and helpers which are used by multiple subcommands related to
//! creating deploys.

use std::{convert::TryInto, fs, io, path::PathBuf, str::FromStr};

use serde::{self, Deserialize};

use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_node::{
    crypto::{
        asymmetric_key::{PublicKey as NodePublicKey, SecretKey},
        hash::Digest,
    },
    types::{DeployHash, TimeDiff, Timestamp},
};
use casper_types::{
    bytesrepr, CLType, CLValue, ContractHash, Key, NamedArg, RuntimeArgs, UIntParseError, URef,
    U512,
};

use crate::{
    cl_type,
    deploy::DeployParams,
    error::{Error, Result},
    help, ExecutableDeployItemExt, TransferTarget,
};

pub(super) fn none_if_empty(value: &'_ str) -> Option<&'_ str> {
    if value.is_empty() {
        return None;
    }
    Some(value)
}

fn timestamp(value: &str) -> Result<Timestamp> {
    if value.is_empty() {
        return Ok(Timestamp::now());
    }
    Timestamp::from_str(value).map_err(|error| Error::FailedToParseTimestamp("timestamp", error))
}

fn ttl(value: &str) -> Result<TimeDiff> {
    TimeDiff::from_str(value).map_err(|error| Error::FailedToParseTimeDiff("ttl", error))
}

fn gas_price(value: &str) -> Result<u64> {
    Ok(value
        .parse::<u64>()
        .map_err(|error| Error::FailedToParseInt("gas_price", error))?)
}

fn dependencies(values: &[&str]) -> Result<Vec<DeployHash>> {
    let mut hashes = Vec::with_capacity(values.len());
    for value in values {
        let digest = Digest::from_hex(value)?;
        hashes.push(DeployHash::new(digest))
    }
    Ok(hashes)
}

/// Handles providing the arg for and retrieval of simple session and payment args.
mod arg_simple {
    use super::*;

    const ARG_VALUE_NAME: &str = r#""NAME:TYPE='VALUE'" OR "NAME:TYPE=null""#;

    pub(crate) mod session {
        use super::*;

        pub fn parse(values: &[&str]) -> Result<Option<RuntimeArgs>> {
            Ok(if values.is_empty() {
                None
            } else {
                Some(get(values)?)
            })
        }
    }

    pub(crate) mod payment {
        use super::*;

        pub fn parse(values: &[&str]) -> Result<Option<RuntimeArgs>> {
            Ok(if values.is_empty() {
                None
            } else {
                Some(get(values)?)
            })
        }
    }

    fn get(values: &[&str]) -> Result<RuntimeArgs> {
        let mut runtime_args = RuntimeArgs::new();
        for arg in values {
            let parts = split_arg(arg)?;
            parts_to_cl_value(parts, &mut runtime_args)?;
        }
        Ok(runtime_args)
    }

    /// Splits a single arg of the form `NAME:TYPE='VALUE'` into its constituent parts.
    fn split_arg(arg: &str) -> Result<(&str, CLType, &str)> {
        let parts: Vec<_> = arg.splitn(3, &[':', '='][..]).collect();
        if parts.len() != 3 {
            return Err(Error::InvalidCLValue(format!(
                "arg {} should be formatted as {}",
                arg, ARG_VALUE_NAME
            )));
        }
        let cl_type = cl_type::parse(&parts[1]).map_err(|_| {
            Error::InvalidCLValue(format!(
                "unknown variant {}, expected one of {}",
                parts[1],
                help::supported_cl_type_list()
            ))
        })?;
        Ok((parts[0], cl_type, parts[2]))
    }

    /// Insert a value built from a single arg which has been split into its constituent parts.
    fn parts_to_cl_value(
        parts: (&str, CLType, &str),
        runtime_args: &mut RuntimeArgs,
    ) -> Result<()> {
        let (name, cl_type, value) = parts;
        let cl_value = cl_type::parts_to_cl_value(cl_type, value)?;
        runtime_args.insert_cl_value(name, cl_value);
        Ok(())
    }
}

/// Handles providing the arg for and retrieval of complex session and payment args. These are read
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
                return Err(Error::InvalidArgument("session_path", path.to_string()));
            }
            get(path).map_err(|error| Error::IoError {
                context: format!("error reading session file at '{}'", path),
                error,
            })
        }
    }

    pub mod payment {
        use super::*;

        pub fn parse(path: &str) -> Result<RuntimeArgs> {
            if path.is_empty() {
                return Err(Error::InvalidArgument("payment_path", path.to_string()));
            }
            get(path).map_err(|error| Error::IoError {
                context: format!("error reading payment file at '{}'", path),
                error,
            })
        }
    }

    fn get(path: &str) -> io::Result<RuntimeArgs> {
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

const STANDARD_PAYMENT_ARG_NAME: &str = "amount";
fn standard_payment(value: &str) -> Result<RuntimeArgs> {
    let arg = U512::from_dec_str(value)
        .map_err(|err| Error::FailedToParseUint("amount", UIntParseError::FromDecStr(err)))?;
    let mut runtime_args = RuntimeArgs::new();
    runtime_args.insert(STANDARD_PAYMENT_ARG_NAME, arg);
    Ok(runtime_args)
}

pub(crate) fn secret_key(value: &str) -> Result<SecretKey> {
    let path = PathBuf::from(value);
    SecretKey::from_file(path).map_err(Error::CryptoError)
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
    let secret_key = self::secret_key(secret_key)?;
    let timestamp = self::timestamp(timestamp)?;
    let ttl = self::ttl(ttl)?;
    let gas_price = self::gas_price(gas_price)?;
    let dependencies = self::dependencies(dependencies)?;
    let chain_name = chain_name.to_string();

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
    session_package_hash: &str,
    session_package_name: &str,
    session_path: &str,
    session_args: &[&str],
    session_args_complex: &str,
    session_version: &str,
    session_entry_point: &str,
) -> Result<ExecutableDeployItem> {
    let session_args = args_from_simple_or_complex(
        arg_simple::session::parse(session_args)?,
        args_complex::session::parse(session_args_complex).ok(),
    );
    let invalid_entry_point =
        || Error::InvalidArgument("session_entry_point", session_entry_point.to_string());
    if let Some(session_name) = name(session_name) {
        return ExecutableDeployItem::new_stored_contract_by_name(
            session_name,
            entry_point(session_entry_point).ok_or_else(invalid_entry_point)?,
            session_args,
        );
    }

    if let Ok(session_hash) = hash(session_hash) {
        return ExecutableDeployItem::new_stored_contract_by_hash(
            session_hash,
            entry_point(session_entry_point).ok_or_else(invalid_entry_point)?,
            session_args,
        );
    }

    let version = version(session_version).ok();
    if let Some(package_name) = name(session_package_name) {
        return ExecutableDeployItem::new_stored_versioned_contract_by_name(
            package_name,
            version,
            entry_point(session_entry_point).ok_or_else(invalid_entry_point)?,
            session_args,
        );
    }

    if let Ok(package_hash) = hash(session_package_hash) {
        return ExecutableDeployItem::new_stored_versioned_contract_by_hash(
            package_hash,
            version,
            entry_point(session_entry_point).ok_or_else(invalid_entry_point)?,
            session_args,
        );
    }

    let module_bytes = fs::read(session_path).map_err(|error| Error::IoError {
        context: format!("unable to read session file at '{}'", session_path),
        error,
    })?;
    ExecutableDeployItem::new_module_bytes(module_bytes, session_args)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn parse_payment_info(
    standard_payment_amount: &str,
    payment_hash: &str,
    payment_name: &str,
    payment_package_hash: &str,
    payment_package_name: &str,
    payment_path: &str,
    payment_args: &[&str],
    payment_args_complex: &str,
    payment_version: &str,
    payment_entry_point: &str,
) -> Result<ExecutableDeployItem> {
    if let Ok(payment_args) = standard_payment(standard_payment_amount) {
        return ExecutableDeployItem::new_module_bytes(vec![], payment_args);
    }

    let invalid_entry_point =
        || Error::InvalidArgument("payment_entry_point", payment_entry_point.to_string());

    let payment_args = args_from_simple_or_complex(
        arg_simple::payment::parse(payment_args)?,
        args_complex::payment::parse(payment_args_complex).ok(),
    );

    if let Some(payment_name) = name(payment_name) {
        return ExecutableDeployItem::new_stored_contract_by_name(
            payment_name,
            entry_point(payment_entry_point).ok_or_else(invalid_entry_point)?,
            payment_args,
        );
    }

    if let Ok(payment_hash) = hash(payment_hash) {
        return ExecutableDeployItem::new_stored_contract_by_hash(
            payment_hash,
            entry_point(payment_entry_point).ok_or_else(invalid_entry_point)?,
            payment_args,
        );
    }

    let version = version(payment_version).ok();
    if let Some(package_name) = name(payment_package_name) {
        return ExecutableDeployItem::new_stored_versioned_contract_by_name(
            package_name,
            version,
            entry_point(payment_entry_point).ok_or_else(invalid_entry_point)?,
            payment_args,
        );
    }

    if let Ok(package_hash) = hash(payment_package_hash) {
        return ExecutableDeployItem::new_stored_versioned_contract_by_hash(
            package_hash,
            version,
            entry_point(payment_entry_point).ok_or_else(invalid_entry_point)?,
            payment_args,
        );
    }

    let module_bytes = fs::read(payment_path).map_err(|error| Error::IoError {
        context: format!("unable to read payment file at '{}'", payment_path),
        error,
    })?;
    ExecutableDeployItem::new_module_bytes(module_bytes, payment_args)
}

pub(crate) fn get_transfer_target(
    target_account: &str,
    target_purse: &str,
) -> Result<TransferTarget> {
    if target_account.is_empty() {
        let purse = purse(target_purse)?;
        Ok(TransferTarget::OwnPurse(purse))
    } else if target_purse.is_empty() {
        let account = account(target_account)?;
        Ok(TransferTarget::Account(account))
    } else {
        Err(Error::InvalidArgument(
            "target_account | target_purse",
            "Invalid arguments to get_transfer_target - must provide either a target account or purse.".to_string()
        ))
    }
}

pub(crate) fn output(value: &str) -> Option<&str> {
    none_if_empty(value)
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

fn hash(value: &str) -> Result<ContractHash> {
    parse_contract_hash(value)
}

fn name(value: &str) -> Option<String> {
    none_if_empty(value).map(str::to_string)
}

fn entry_point(value: &str) -> Option<String> {
    none_if_empty(value).map(str::to_string)
}

fn version(value: &str) -> Result<u32> {
    Ok(value
        .parse::<u32>()
        .map_err(|error| Error::FailedToParseInt("version", error))?)
}

fn account(value: &str) -> Result<NodePublicKey> {
    Ok(NodePublicKey::from_hex(value)?)
}

pub(crate) fn purse(value: &str) -> Result<URef> {
    Ok(
        URef::from_formatted_str(value)
            .map_err(|error| Error::FailedToParseURef("purse", error))?,
    )
}

#[cfg(test)]
mod tests {

    use super::*;

    use std::convert::TryFrom;

    use casper_types::{
        account::AccountHash, bytesrepr::ToBytes, AccessRights, CLTyped, CLValue, NamedArg,
        PublicKey, RuntimeArgs, U128, U256, U512,
    };

    fn valid_simple_args_test<T: CLTyped + ToBytes>(cli_string: &str, expected: T) {
        let expected = Some(RuntimeArgs::from(vec![NamedArg::new(
            "x".to_string(),
            CLValue::from_t(expected).unwrap(),
        )]));

        assert_eq!(
            arg_simple::payment::parse(&[cli_string]).expect("should parse"),
            expected
        );
        assert_eq!(
            arg_simple::session::parse(&[cli_string]).expect("should parse"),
            expected
        );
    }

    #[test]
    fn should_parse_bool_via_args_simple() {
        valid_simple_args_test("x:bool='f'", false);
        valid_simple_args_test("x:bool='false'", false);
        valid_simple_args_test("x:bool='t'", true);
        valid_simple_args_test("x:bool='true'", true);
        valid_simple_args_test("x:opt_bool='f'", Some(false));
        valid_simple_args_test("x:opt_bool='t'", Some(true));
        valid_simple_args_test::<Option<bool>>("x:opt_bool=null", None);
    }

    #[test]
    fn should_parse_i32_via_args_simple() {
        valid_simple_args_test("x:i32='2147483647'", i32::max_value());
        valid_simple_args_test("x:i32='0'", 0_i32);
        valid_simple_args_test("x:i32='-2147483648'", i32::min_value());
        valid_simple_args_test("x:opt_i32='-1'", Some(-1_i32));
        valid_simple_args_test::<Option<i32>>("x:opt_i32=null", None);
    }

    #[test]
    fn should_parse_i64_via_args_simple() {
        valid_simple_args_test("x:i64='9223372036854775807'", i64::max_value());
        valid_simple_args_test("x:i64='0'", 0_i64);
        valid_simple_args_test("x:i64='-9223372036854775808'", i64::min_value());
        valid_simple_args_test("x:opt_i64='-1'", Some(-1_i64));
        valid_simple_args_test::<Option<i64>>("x:opt_i64=null", None);
    }

    #[test]
    fn should_parse_u8_via_args_simple() {
        valid_simple_args_test("x:u8='0'", 0_u8);
        valid_simple_args_test("x:u8='255'", u8::max_value());
        valid_simple_args_test("x:opt_u8='1'", Some(1_u8));
        valid_simple_args_test::<Option<u8>>("x:opt_u8=null", None);
    }

    #[test]
    fn should_parse_u32_via_args_simple() {
        valid_simple_args_test("x:u32='0'", 0_u32);
        valid_simple_args_test("x:u32='4294967295'", u32::max_value());
        valid_simple_args_test("x:opt_u32='1'", Some(1_u32));
        valid_simple_args_test::<Option<u32>>("x:opt_u32=null", None);
    }

    #[test]
    fn should_parse_u64_via_args_simple() {
        valid_simple_args_test("x:u64='0'", 0_u64);
        valid_simple_args_test("x:u64='18446744073709551615'", u64::max_value());
        valid_simple_args_test("x:opt_u64='1'", Some(1_u64));
        valid_simple_args_test::<Option<u64>>("x:opt_u64=null", None);
    }

    #[test]
    fn should_parse_u128_via_args_simple() {
        valid_simple_args_test("x:u128='0'", U128::zero());
        valid_simple_args_test(
            "x:u128='340282366920938463463374607431768211455'",
            U128::max_value(),
        );
        valid_simple_args_test("x:opt_u128='1'", Some(U128::from(1)));
        valid_simple_args_test::<Option<U128>>("x:opt_u128=null", None);
    }

    #[test]
    fn should_parse_u256_via_args_simple() {
        valid_simple_args_test("x:u256='0'", U256::zero());
        valid_simple_args_test(
            "x:u256='115792089237316195423570985008687907853269984665640564039457584007913129639935'",
            U256::max_value(),
        );
        valid_simple_args_test("x:opt_u256='1'", Some(U256::from(1)));
        valid_simple_args_test::<Option<U256>>("x:opt_u256=null", None);
    }

    #[test]
    fn should_parse_u512_via_args_simple() {
        valid_simple_args_test("x:u512='0'", U512::zero());
        valid_simple_args_test(
            "x:u512='134078079299425970995740249982058461274793658205923933777235614437217640300735\
            46976801874298166903427690031858186486050853753882811946569946433649006084095'",
            U512::max_value(),
        );
        valid_simple_args_test("x:opt_u512='1'", Some(U512::from(1)));
        valid_simple_args_test::<Option<U512>>("x:opt_u512=null", None);
    }

    #[test]
    fn should_parse_unit_via_args_simple() {
        valid_simple_args_test("x:unit=''", ());
        valid_simple_args_test("x:opt_unit=''", Some(()));
        valid_simple_args_test::<Option<()>>("x:opt_unit=null", None);
    }

    #[test]
    fn should_parse_string_via_args_simple() {
        let value = String::from("test string");
        valid_simple_args_test(&format!("x:string='{}'", value), value.clone());
        valid_simple_args_test(&format!("x:opt_string='{}'", value), Some(value));
        valid_simple_args_test::<Option<String>>("x:opt_string=null", None);
    }

    #[test]
    fn should_parse_key_via_args_simple() {
        let bytes = (1..33).collect::<Vec<_>>();
        let array = <[u8; 32]>::try_from(bytes.as_ref()).unwrap();

        let key_account = Key::Account(AccountHash::new(array));
        let key_hash = Key::Hash(array);
        let key_uref = Key::URef(URef::new(array, AccessRights::NONE));

        for key in &[key_account, key_hash, key_uref] {
            valid_simple_args_test(&format!("x:key='{}'", key.to_formatted_string()), *key);
            valid_simple_args_test(
                &format!("x:opt_key='{}'", key.to_formatted_string()),
                Some(*key),
            );
            valid_simple_args_test::<Option<Key>>("x:opt_key=null", None);
        }
    }

    #[test]
    fn should_parse_account_hash_via_args_simple() {
        let bytes = (1..33).collect::<Vec<_>>();
        let array = <[u8; 32]>::try_from(bytes.as_ref()).unwrap();
        let value = AccountHash::new(array);
        valid_simple_args_test(
            &format!("x:account_hash='{}'", value.to_formatted_string()),
            value,
        );
        valid_simple_args_test(
            &format!("x:opt_account_hash='{}'", value.to_formatted_string()),
            Some(value),
        );
        valid_simple_args_test::<Option<AccountHash>>("x:opt_account_hash=null", None);
    }

    #[test]
    fn should_parse_uref_via_args_simple() {
        let bytes = (1..33).collect::<Vec<_>>();
        let array = <[u8; 32]>::try_from(bytes.as_ref()).unwrap();
        let value = URef::new(array, AccessRights::READ_ADD_WRITE);
        valid_simple_args_test(&format!("x:uref='{}'", value.to_formatted_string()), value);
        valid_simple_args_test(
            &format!("x:opt_uref='{}'", value.to_formatted_string()),
            Some(value),
        );
        valid_simple_args_test::<Option<URef>>("x:opt_uref=null", None);
    }

    #[test]
    fn should_parse_public_key_via_args_simple() {
        let hex_value = "0119bf44096984cdfe8541bac167dc3b96c85086aa30b6b6cb0c5c38ad703166e1";
        let value = PublicKey::from(NodePublicKey::from_hex(hex_value).unwrap());
        valid_simple_args_test(&format!("x:public_key='{}'", hex_value), value);
        valid_simple_args_test(&format!("x:opt_public_key='{}'", hex_value), Some(value));
        valid_simple_args_test::<Option<PublicKey>>("x:opt_public_key=null", None);
    }
}
