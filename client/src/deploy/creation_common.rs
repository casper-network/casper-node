//! This module contains structs and helpers which are used by multiple subcommands related to
//! creating deploys.

use std::{
    convert::{TryFrom, TryInto},
    process,
    str::FromStr,
};

use clap::{App, AppSettings, Arg, ArgGroup, ArgMatches};
use lazy_static::lazy_static;
use serde::{self, Deserialize};

use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_node::{
    crypto::hash::Digest,
    rpcs::account::PutDeployParams,
    types::{Deploy, TimeDiff, Timestamp},
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
    AccessRights, CLType, CLValue, Key, NamedArg, RuntimeArgs, URef, U128, U256, U512,
};

use crate::common;

/// This struct defines the order in which the args are shown for this subcommand's help message.
pub(super) enum DisplayOrder {
    ShowArgExamples,
    NodeAddress,
    SecretKey,
    TransferAmount,
    TransferSourcePurse,
    TransferTargetAccount,
    TransferTargetPurse,
    Timestamp,
    Ttl,
    GasPrice,
    Dependencies,
    ChainName,
    SessionCode,
    SessionArgSimple,
    SessionArgsComplex,
    StandardPayment,
    PaymentCode,
    PaymentArgSimple,
    PaymentArgsComplex,
}

/// Handles providing the arg for and executing the show-arg-examples option.
pub(super) mod show_arg_examples {
    use super::*;

    pub(in crate::deploy) const ARG_NAME: &str = "show-arg-examples";
    const ARG_SHORT: &str = "e";
    const ARG_HELP: &str =
        "If passed, all other options are ignored and a set of examples of session-/payment-args \
        is printed";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .help(ARG_HELP)
            .display_order(DisplayOrder::ShowArgExamples as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> bool {
        if !matches.is_present(ARG_NAME) {
            return false;
        }

        let bytes = (1..33).collect::<Vec<_>>();
        let array = <[u8; 32]>::try_from(bytes.as_ref()).unwrap();

        println!("Examples for passing values via --session-arg or --payment-arg:");
        println!("name_01:bool='false'");
        println!("name_02:i32='-1'");
        println!("name_03:i64='-2'");
        println!("name_04:u8='3'");
        println!("name_05:u32='4'");
        println!("name_06:u64='5'");
        println!("name_07:u128='6'");
        println!("name_08:u256='7'");
        println!("name_09:u512='8'");
        println!("name_10:unit=''");
        println!("name_11:string='a value'");
        println!(
            "key_account_name:key='{}'",
            Key::Account(AccountHash::new(array)).to_formatted_string()
        );
        println!(
            "key_hash_name:key='{}'",
            Key::Hash(array).to_formatted_string()
        );
        println!(
            "key_uref_name:key='{}'",
            Key::URef(URef::new(array, AccessRights::NONE)).to_formatted_string()
        );
        println!(
            "uref_name:uref='{}'",
            URef::new(array, AccessRights::READ_ADD_WRITE).to_formatted_string()
        );

        true
    }
}

/// Handles providing the arg for and retrieval of the timestamp.
pub(super) mod timestamp {
    use super::*;

    const ARG_NAME: &str = "timestamp";
    const ARG_VALUE_NAME: &str = "MILLISECONDS";
    const ARG_HELP: &str =
        "Timestamp as the number of milliseconds since the Unix epoch. If not provided, the \
        current time will be used";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Timestamp as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> Timestamp {
        matches
            .value_of(ARG_NAME)
            .map_or_else(Timestamp::now, |value| {
                Timestamp::from_str(value)
                    .unwrap_or_else(|error| panic!("should parse {}: {}", ARG_NAME, error))
            })
    }
}

/// Handles providing the arg for and retrieval of the time to live.
pub(super) mod ttl {
    use super::*;

    const ARG_NAME: &str = "ttl";
    const ARG_VALUE_NAME: &str = "MILLISECONDS";
    const ARG_DEFAULT: &str = "3600000";
    const ARG_HELP: &str =
        "Time (in milliseconds) that the deploy will remain valid for. A deploy can only be \
        included in a block between `timestamp` and `timestamp + ttl`.";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .default_value(ARG_DEFAULT)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Ttl as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> TimeDiff {
        TimeDiff::from_str(
            matches
                .value_of(ARG_NAME)
                .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME)),
        )
        .unwrap_or_else(|error| panic!("should parse {}: {}", ARG_NAME, error))
    }
}

/// Handles providing the arg for and retrieval of the gas price.
pub(super) mod gas_price {
    use super::*;

    const ARG_NAME: &str = "gas-price";
    const ARG_VALUE_NAME: &str = "INTEGER";
    const ARG_DEFAULT: &str = "10";
    const ARG_HELP: &str =
        "Conversion rate between the cost of Wasm opcodes and the motes sent by the payment code";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .default_value(ARG_DEFAULT)
            .help(ARG_HELP)
            .display_order(DisplayOrder::GasPrice as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> u64 {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .parse()
            .unwrap_or_else(|error| panic!("should parse {}: {}", ARG_NAME, error))
    }
}

/// Handles providing the arg for and retrieval of the deploy dependencies.
pub(super) mod dependencies {
    use super::*;
    use casper_node::types::DeployHash;

    const ARG_NAME: &str = "dependency";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str =
        "A hex-encoded deploy hash of a deploy which must be executed before this deploy";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .multiple(true)
            .value_name(ARG_VALUE_NAME)
            .takes_value(true)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Dependencies as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> Vec<DeployHash> {
        matches
            .values_of(ARG_NAME)
            .map(|values| {
                values
                    .map(|hex_hash| {
                        let digest = Digest::from_hex(hex_hash).unwrap_or_else(|error| {
                            panic!(
                                "could not parse --{} {} as hex-encoded deploy hash: {}",
                                ARG_NAME, hex_hash, error
                            )
                        });
                        DeployHash::new(digest)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Handles providing the arg for and retrieval of the chain name.
pub(super) mod chain_name {
    use super::*;

    const ARG_NAME: &str = "chain-name";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_DEFAULT: &str = "casper-example";
    const ARG_HELP: &str =
        "Name of the chain, to avoid the deploy from being accidentally or maliciously included in \
        a different chain";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .default_value(ARG_DEFAULT)
            .help(ARG_HELP)
            .display_order(DisplayOrder::ChainName as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

/// Handles providing the arg for and retrieval of the session code bytes.
pub(super) mod session {
    use super::*;

    const ARG_NAME: &str = "session-path";
    const ARG_SHORT: &str = "s";
    const ARG_VALUE_NAME: &str = "PATH";
    const ARG_HELP: &str = "Path to the compiled Wasm session code";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .short(ARG_SHORT)
            .long(ARG_NAME)
            .required_unless(show_arg_examples::ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::SessionCode as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> Vec<u8> {
        common::read_file(
            matches
                .value_of(ARG_NAME)
                .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME)),
        )
    }
}

/// Handles providing the arg for and retrieval of simple session and payment args.
pub(super) mod arg_simple {
    use super::*;

    const ARG_VALUE_NAME: &str = "NAME:TYPE='VALUE'";

    lazy_static! {
        static ref SUPPORTED_TYPES: Vec<(&'static str, CLType)> = vec![
            ("bool", CLType::Bool),
            ("i32", CLType::I32),
            ("i64", CLType::I64),
            ("u8", CLType::U8),
            ("u32", CLType::U32),
            ("u64", CLType::U64),
            ("u128", CLType::U128),
            ("u256", CLType::U256),
            ("u512", CLType::U512),
            ("unit", CLType::Unit),
            ("string", CLType::String),
            ("key", CLType::Key),
            ("uref", CLType::URef),
        ];
        static ref SUPPORTED_LIST: String = {
            let mut msg = String::new();
            for (index, item) in SUPPORTED_TYPES.iter().map(|(name, _)| name).enumerate() {
                msg.push_str(item);
                if index < SUPPORTED_TYPES.len() - 1 {
                    msg.push_str(", ")
                }
            }
            msg
        };
        static ref ARG_HELP: String = format!(
            "For simple CLTypes, a named and typed arg which is passed to the Wasm code. To see \
            an example for each type, run '--{}'. This arg can be repeated to pass multiple named, \
            typed args, but can only be used for the following types: {}",
            super::show_arg_examples::ARG_NAME,
            *SUPPORTED_LIST
        );
    }

    pub(in crate::deploy) mod session {
        use super::*;

        pub const ARG_NAME: &str = "session-arg";
        const ARG_SHORT: &str = "a";

        pub fn arg() -> Arg<'static, 'static> {
            super::arg(ARG_NAME, DisplayOrder::SessionArgSimple as usize)
                .short(ARG_SHORT)
                .requires(super::session::ARG_NAME)
        }

        pub fn get(matches: &ArgMatches) -> Option<RuntimeArgs> {
            super::get(matches, ARG_NAME)
        }
    }

    pub(in crate::deploy) mod payment {
        use super::*;

        pub const ARG_NAME: &str = "payment-arg";

        pub fn arg() -> Arg<'static, 'static> {
            super::arg(ARG_NAME, DisplayOrder::PaymentArgSimple as usize)
                .requires(super::payment::ARG_NAME)
        }

        pub fn get(matches: &ArgMatches) -> Option<RuntimeArgs> {
            super::get(matches, ARG_NAME)
        }
    }

    fn arg(name: &'static str, order: usize) -> Arg<'static, 'static> {
        Arg::with_name(name)
            .long(name)
            .required(false)
            .multiple(true)
            .value_name(ARG_VALUE_NAME)
            .help(&*ARG_HELP)
            .display_order(order)
    }

    fn get(matches: &ArgMatches, name: &str) -> Option<RuntimeArgs> {
        let args = matches.values_of(name)?;
        let mut runtime_args = RuntimeArgs::new();
        for arg in args {
            let parts = split_arg(arg);
            parts_to_cl_value(parts, &mut runtime_args);
        }
        Some(runtime_args)
    }

    /// Splits a single arg of the form `NAME:TYPE='VALUE'` into its constituent parts.
    fn split_arg(arg: &str) -> (&str, CLType, &str) {
        let parts: Vec<_> = arg.splitn(3, &[':', '='][..]).collect();
        if parts.len() != 3 {
            panic!("arg {} should be formatted as {}", arg, ARG_VALUE_NAME);
        }
        let cl_type = match parts[1].to_lowercase() {
            t if t == SUPPORTED_TYPES[0].0 => SUPPORTED_TYPES[0].1.clone(),
            t if t == SUPPORTED_TYPES[1].0 => SUPPORTED_TYPES[1].1.clone(),
            t if t == SUPPORTED_TYPES[2].0 => SUPPORTED_TYPES[2].1.clone(),
            t if t == SUPPORTED_TYPES[3].0 => SUPPORTED_TYPES[3].1.clone(),
            t if t == SUPPORTED_TYPES[4].0 => SUPPORTED_TYPES[4].1.clone(),
            t if t == SUPPORTED_TYPES[5].0 => SUPPORTED_TYPES[5].1.clone(),
            t if t == SUPPORTED_TYPES[6].0 => SUPPORTED_TYPES[6].1.clone(),
            t if t == SUPPORTED_TYPES[7].0 => SUPPORTED_TYPES[7].1.clone(),
            t if t == SUPPORTED_TYPES[8].0 => SUPPORTED_TYPES[8].1.clone(),
            t if t == SUPPORTED_TYPES[9].0 => SUPPORTED_TYPES[9].1.clone(),
            t if t == SUPPORTED_TYPES[10].0 => SUPPORTED_TYPES[10].1.clone(),
            t if t == SUPPORTED_TYPES[11].0 => SUPPORTED_TYPES[11].1.clone(),
            t if t == SUPPORTED_TYPES[12].0 => SUPPORTED_TYPES[12].1.clone(),
            _ => panic!(
                "unknown variant {}, expected one of {}",
                parts[1], *SUPPORTED_LIST
            ),
        };
        (parts[0], cl_type, parts[2].trim_matches('\''))
    }

    /// Insert a value built from a single arg which has been split into its constituent parts.
    fn parts_to_cl_value(parts: (&str, CLType, &str), runtime_args: &mut RuntimeArgs) {
        let (name, cl_type, value) = parts;
        let cl_value = match cl_type {
            CLType::Bool => match value.to_lowercase().as_str() {
                "true" | "t" => CLValue::from_t(true).unwrap(),
                "false" | "f" => CLValue::from_t(false).unwrap(),
                invalid => panic!(
                    "can't parse {} as a bool.  Should be 'true' or 'false'",
                    invalid
                ),
            },
            CLType::I32 => {
                let x = i32::from_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as i32: {}", value, error));
                CLValue::from_t(x).unwrap()
            }
            CLType::I64 => {
                let x = i64::from_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as i64: {}", value, error));
                CLValue::from_t(x).unwrap()
            }
            CLType::U8 => {
                let x = u8::from_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as u8: {}", value, error));
                CLValue::from_t(x).unwrap()
            }
            CLType::U32 => {
                let x = u32::from_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as u32: {}", value, error));
                CLValue::from_t(x).unwrap()
            }
            CLType::U64 => {
                let x = u64::from_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as u64: {}", value, error));
                CLValue::from_t(x).unwrap()
            }
            CLType::U128 => {
                let x = U128::from_dec_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as U128: {}", value, error));
                CLValue::from_t(x).unwrap()
            }
            CLType::U256 => {
                let x = U256::from_dec_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as U256: {}", value, error));
                CLValue::from_t(x).unwrap()
            }
            CLType::U512 => {
                let x = U512::from_dec_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as U512: {}", value, error));
                CLValue::from_t(x).unwrap()
            }
            CLType::Unit => {
                if !value.is_empty() {
                    panic!("can't parse {} as unit.  Should be ''", value)
                }
                CLValue::from_t(()).unwrap()
            }
            CLType::String => CLValue::from_t(value).unwrap(),
            CLType::Key => {
                let key = Key::from_formatted_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as Key: {:?}", value, error));
                CLValue::from_t(key).unwrap()
            }
            CLType::URef => {
                let uref = URef::from_formatted_str(value)
                    .unwrap_or_else(|error| panic!("can't parse {} as URef: {:?}", value, error));
                CLValue::from_t(uref).unwrap()
            }
            _ => unreachable!(),
        };
        runtime_args.insert_cl_value(name, cl_value);
    }
}

/// Handles providing the arg for and retrieval of complex session and payment args.  These are read
/// in from a file.
pub(super) mod args_complex {
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

    const ARG_VALUE_NAME: &str = "PATH";
    const ARG_HELP: &str =
        "Path to file containing named and typed args for passing to the Wasm code";

    pub(in crate::deploy) mod session {
        use super::*;

        pub const ARG_NAME: &str = "session-args-complex";

        pub fn arg() -> Arg<'static, 'static> {
            super::arg(ARG_NAME, DisplayOrder::SessionArgsComplex as usize)
                .requires(super::session::ARG_NAME)
        }

        pub fn get(matches: &ArgMatches) -> Option<RuntimeArgs> {
            super::get(matches, ARG_NAME)
        }
    }

    pub(in crate::deploy) mod payment {
        use super::*;

        pub const ARG_NAME: &str = "payment-args-complex";

        pub fn arg() -> Arg<'static, 'static> {
            super::arg(ARG_NAME, DisplayOrder::PaymentArgsComplex as usize)
                .requires(super::payment::ARG_NAME)
        }

        pub fn get(matches: &ArgMatches) -> Option<RuntimeArgs> {
            super::get(matches, ARG_NAME)
        }
    }

    fn arg(name: &'static str, order: usize) -> Arg<'static, 'static> {
        Arg::with_name(name)
            .long(name)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(order)
    }

    fn get(matches: &ArgMatches, name: &str) -> Option<RuntimeArgs> {
        let path = matches.value_of(name)?;
        let bytes = common::read_file(path);
        // Received structured args in JSON format.
        let args: Vec<DeployArg> = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("should parse {} as Vec<DeployArg>: {}", path, error));
        // Convert JSON deploy args into vector of named args.
        let named_args = args
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<NamedArg>, _>>()
            .unwrap_or_else(|error| {
                panic!("should deserialize {} as Vec<NamedArg>: {}", path, error)
            });
        Some(RuntimeArgs::from(named_args))
    }
}

/// Handles providing the arg for and retrieval of the payment code bytes.
pub(super) mod payment {
    use super::*;

    pub(in crate::deploy) const ARG_NAME: &str = "payment-path";
    const ARG_VALUE_NAME: &str = "PATH";
    const ARG_HELP: &str = "Path to the compiled Wasm payment code";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::PaymentCode as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> Option<Vec<u8>> {
        let path = matches.value_of(ARG_NAME)?;
        Some(common::read_file(path))
    }
}

/// Handles providing the arg for and retrieval of the payment-amount arg.
pub(super) mod standard_payment {
    use super::*;

    const STANDARD_PAYMENT_ARG_NAME: &str = "amount";

    pub(in crate::deploy) const ARG_NAME: &str = "payment-amount";
    const ARG_VALUE_NAME: &str = "AMOUNT";
    const ARG_SHORT: &str = "p";
    const ARG_HELP: &str =
        "If provided, uses the standard-payment system contract rather than custom payment Wasm. \
        The value is the 'amount' arg of the standard-payment contract. This arg is incompatible \
        with all other --payment-xxx args";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::StandardPayment as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> Option<RuntimeArgs> {
        let arg = U512::from_dec_str(matches.value_of(ARG_NAME)?)
            .unwrap_or_else(|error| panic!("should parse {} as U512: {}", ARG_NAME, error));

        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert(STANDARD_PAYMENT_ARG_NAME, arg);
        Some(runtime_args)
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

pub(super) fn parse_session_info(matches: &ArgMatches<'_>) -> ExecutableDeployItem {
    let module_bytes = session::get(matches);
    let session_args = args_from_simple_or_complex(
        arg_simple::session::get(matches),
        args_complex::session::get(matches),
    );

    ExecutableDeployItem::ModuleBytes {
        module_bytes,
        args: session_args.to_bytes().expect("should serialize"),
    }
}

pub(super) fn parse_payment_info(matches: &ArgMatches<'_>) -> ExecutableDeployItem {
    // If we're using the standard-payment system contract, just return empty module bytes.
    if let Some(payment_args) = standard_payment::get(matches) {
        return ExecutableDeployItem::ModuleBytes {
            module_bytes: vec![],
            args: payment_args.to_bytes().expect("should serialize"),
        };
    }

    // Get the payment code and args options.
    let module_bytes = payment::get(matches).expect("should have payment-amount or payment-path");
    let payment_args = args_from_simple_or_complex(
        arg_simple::payment::get(matches),
        args_complex::payment::get(matches),
    );

    ExecutableDeployItem::ModuleBytes {
        module_bytes,
        args: payment_args.to_bytes().expect("should serialize"),
    }
}

pub(super) fn apply_common_creation_options<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
    subcommand
        .setting(AppSettings::NextLineHelp)
        .arg(show_arg_examples::arg())
        .arg(
            common::node_address::arg(DisplayOrder::NodeAddress as usize)
                .required_unless(show_arg_examples::ARG_NAME),
        )
        .arg(
            common::secret_key::arg(DisplayOrder::SecretKey as usize)
                .required_unless(show_arg_examples::ARG_NAME),
        )
        .arg(timestamp::arg())
        .arg(ttl::arg())
        .arg(gas_price::arg())
        .arg(dependencies::arg())
        .arg(chain_name::arg())
        .arg(standard_payment::arg())
        .arg(payment::arg())
        .arg(arg_simple::payment::arg())
        .arg(args_complex::payment::arg())
        // Group the payment-arg args so only one style is used to ensure consistent ordering.
        .group(
            ArgGroup::with_name("payment-args")
                .arg(arg_simple::payment::ARG_NAME)
                .arg(args_complex::payment::ARG_NAME)
                .required(false),
        )
        // Group payment-amount, payment-path and show-arg-examples so that we can require only
        // one of these.
        .group(
            ArgGroup::with_name("required-payment-options")
                .arg(standard_payment::ARG_NAME)
                .arg(payment::ARG_NAME)
                .arg(show_arg_examples::ARG_NAME)
                .required(true),
        )
}

pub(super) fn construct_deploy(
    matches: &ArgMatches<'_>,
    session: ExecutableDeployItem,
) -> PutDeployParams {
    // If we printed the arg examples, exit the process.
    if show_arg_examples::get(matches) {
        process::exit(0);
    }

    let secret_key = common::secret_key::get(matches);
    let timestamp = timestamp::get(matches);
    let ttl = ttl::get(matches);
    let gas_price = gas_price::get(matches);
    let dependencies = dependencies::get(matches);
    let chain_name = chain_name::get(matches);

    let mut rng = rand::thread_rng();

    let payment = parse_payment_info(matches);

    let deploy = Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
        &mut rng,
    );

    let deploy = deploy
        .to_json()
        .as_object()
        .unwrap_or_else(|| panic!("should encode to JSON object type"))
        .clone();
    PutDeployParams { deploy }
}
