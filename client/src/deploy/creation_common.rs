//! This module contains structs and helpers which are used by multiple subcommands related to
//! creating deploys.

use std::{
    convert::{TryFrom, TryInto},
    fs::File,
    io::{self, Write},
    process,
    str::FromStr,
};

use clap::{App, AppSettings, Arg, ArgGroup, ArgMatches};
use lazy_static::lazy_static;
use serde::{self, Deserialize};

use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_node::{
    crypto::{asymmetric_key::PublicKey as NodePublicKey, hash::Digest},
    rpcs::account::PutDeployParams,
    types::{Deploy, TimeDiff, Timestamp},
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
    AccessRights, CLType, CLTyped, CLValue, ContractHash, Key, NamedArg, PublicKey, RuntimeArgs,
    URef, U128, U256, U512,
};

use crate::common;

/// This struct defines the order in which the args are shown for this subcommand's help message.
pub(super) enum DisplayOrder {
    ShowArgExamples,
    Verbose,
    NodeAddress,
    RpcId,
    SecretKey,
    Input,
    Output,
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
    SessionHash,
    SessionName,
    SessionPackageHash,
    SessionPackageName,
    SessionEntryPoint,
    SessionVersion,
    StandardPayment,
    PaymentCode,
    PaymentArgSimple,
    PaymentArgsComplex,
    PaymentHash,
    PaymentName,
    PaymentPackageHash,
    PaymentPackageName,
    PaymentEntryPoint,
    PaymentVersion,
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
            "account_hash_name:account_hash='{}'",
            AccountHash::new(array).to_formatted_string()
        );
        println!(
            "uref_name:uref='{}'",
            URef::new(array, AccessRights::READ_ADD_WRITE).to_formatted_string()
        );
        println!(
            "public_key_name:public_key='{}'",
            NodePublicKey::from_hex(
                "0119bf44096984cdfe8541bac167dc3b96c85086aa30b6b6cb0c5c38ad703166e1"
            )
            .unwrap()
            .to_hex()
        );
        println!("\nOptional values of each of these types can also be specified.");
        println!(
            r#"Prefix the type with "opt_" and use the term "null" without quotes to specify a None value:"#
        );
        println!("name_01:opt_bool='true'       # Some(true)");
        println!("name_02:opt_bool='false'      # Some(false)");
        println!("name_03:opt_bool=null         # None");
        println!("name_04:opt_i32='-1'          # Some(-1)");
        println!("name_05:opt_i32=null          # None");
        println!("name_06:opt_unit=''           # Some(())");
        println!("name_07:opt_unit=null         # None");
        println!("name_08:opt_string='a value'  # Some(\"a value\".to_string())");
        println!("name_09:opt_string='null'     # Some(\"null\".to_string())");
        println!("name_10:opt_string=null       # None");

        true
    }
}

/// Handles providing the arg for and retrieval of the timestamp.
mod timestamp {
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
mod ttl {
    use super::*;

    const ARG_NAME: &str = "ttl";
    const ARG_VALUE_NAME: &str = "DURATION";
    const ARG_DEFAULT: &str = "1hour";
    const ARG_HELP: &str =
        "Time that the deploy will remain valid for. A deploy can only be included in a block \
        between `timestamp` and `timestamp + ttl`. Input examples: '1hr 12min', '30min 50sec', \
        '1day'. For all options, see \
        https://docs.rs/humantime/latest/humantime/fn.parse_duration.html";

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
mod gas_price {
    use super::*;

    const ARG_NAME: &str = "gas-price";
    const ARG_VALUE_NAME: &str = common::ARG_INTEGER;
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
mod dependencies {
    use super::*;
    use casper_node::types::DeployHash;

    const ARG_NAME: &str = "dependency";
    const ARG_VALUE_NAME: &str = common::ARG_HEX_STRING;
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
mod chain_name {
    use super::*;

    const ARG_NAME: &str = "chain-name";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_HELP: &str =
        "Name of the chain, to avoid the deploy from being accidentally or maliciously included in \
        a different chain";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required_unless(show_arg_examples::ARG_NAME)
            .value_name(ARG_VALUE_NAME)
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
mod session_path {
    use super::*;

    pub(super) const ARG_NAME: &str = "session-path";
    const ARG_SHORT: &str = "s";
    const ARG_VALUE_NAME: &str = common::ARG_PATH;
    const ARG_HELP: &str = "Path to the compiled Wasm session code";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .short(ARG_SHORT)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::SessionCode as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> Option<Vec<u8>> {
        matches.value_of(ARG_NAME).map(common::read_file)
    }
}

/// Handles providing the arg for and retrieval of simple session and payment args.
mod arg_simple {
    use super::*;

    const ARG_VALUE_NAME: &str = "NAME:TYPE='VALUE' OR NAME:TYPE=null";

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
            ("account_hash", AccountHash::cl_type()),
            ("uref", CLType::URef),
            ("public_key", CLType::PublicKey),
            ("opt_bool", CLType::Option(Box::new(CLType::Bool))),
            ("opt_i32", CLType::Option(Box::new(CLType::I32))),
            ("opt_i64", CLType::Option(Box::new(CLType::I64))),
            ("opt_u8", CLType::Option(Box::new(CLType::U8))),
            ("opt_u32", CLType::Option(Box::new(CLType::U32))),
            ("opt_u64", CLType::Option(Box::new(CLType::U64))),
            ("opt_u128", CLType::Option(Box::new(CLType::U128))),
            ("opt_u256", CLType::Option(Box::new(CLType::U256))),
            ("opt_u512", CLType::Option(Box::new(CLType::U512))),
            ("opt_unit", CLType::Option(Box::new(CLType::Unit))),
            ("opt_string", CLType::Option(Box::new(CLType::String))),
            ("opt_key", CLType::Option(Box::new(CLType::Key))),
            (
                "opt_account_hash",
                CLType::Option(Box::new(AccountHash::cl_type()))
            ),
            ("opt_uref", CLType::Option(Box::new(CLType::URef))),
            (
                "opt_public_key",
                CLType::Option(Box::new(CLType::PublicKey))
            ),
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
            let (name, cl_type, value) = split_arg(arg);
            let cl_value = parts_to_cl_value(cl_type, value);
            runtime_args.insert_cl_value(name, cl_value);
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
            t if t == SUPPORTED_TYPES[13].0 => SUPPORTED_TYPES[13].1.clone(),
            t if t == SUPPORTED_TYPES[14].0 => SUPPORTED_TYPES[14].1.clone(),
            t if t == SUPPORTED_TYPES[15].0 => SUPPORTED_TYPES[15].1.clone(),
            t if t == SUPPORTED_TYPES[16].0 => SUPPORTED_TYPES[16].1.clone(),
            t if t == SUPPORTED_TYPES[17].0 => SUPPORTED_TYPES[17].1.clone(),
            t if t == SUPPORTED_TYPES[18].0 => SUPPORTED_TYPES[18].1.clone(),
            t if t == SUPPORTED_TYPES[19].0 => SUPPORTED_TYPES[19].1.clone(),
            t if t == SUPPORTED_TYPES[20].0 => SUPPORTED_TYPES[20].1.clone(),
            t if t == SUPPORTED_TYPES[21].0 => SUPPORTED_TYPES[21].1.clone(),
            t if t == SUPPORTED_TYPES[22].0 => SUPPORTED_TYPES[22].1.clone(),
            t if t == SUPPORTED_TYPES[23].0 => SUPPORTED_TYPES[23].1.clone(),
            t if t == SUPPORTED_TYPES[24].0 => SUPPORTED_TYPES[24].1.clone(),
            t if t == SUPPORTED_TYPES[25].0 => SUPPORTED_TYPES[25].1.clone(),
            t if t == SUPPORTED_TYPES[26].0 => SUPPORTED_TYPES[26].1.clone(),
            t if t == SUPPORTED_TYPES[27].0 => SUPPORTED_TYPES[27].1.clone(),
            t if t == SUPPORTED_TYPES[28].0 => SUPPORTED_TYPES[28].1.clone(),
            t if t == SUPPORTED_TYPES[29].0 => SUPPORTED_TYPES[29].1.clone(),
            _ => panic!(
                "unknown variant {}, expected one of {}",
                parts[1], *SUPPORTED_LIST
            ),
        };
        (parts[0], cl_type, parts[2])
    }

    #[derive(PartialEq, Eq)]
    enum OptionalStatus {
        Some,
        None,
        NotOptional,
    }

    /// Parses to a given CLValue taking into account whether the arg represents an optional type or
    /// not.
    fn parse_to_cl_value<T, F>(optional_status: OptionalStatus, parse: F) -> CLValue
    where
        T: CLTyped + ToBytes,
        F: FnOnce() -> T,
    {
        match optional_status {
            OptionalStatus::Some => CLValue::from_t(Some(parse())),
            OptionalStatus::None => CLValue::from_t::<Option<T>>(None),
            OptionalStatus::NotOptional => CLValue::from_t(parse()),
        }
        .unwrap()
    }

    /// Returns a value built from a single arg which has been split into its constituent parts.
    fn parts_to_cl_value(cl_type: CLType, value: &str) -> CLValue {
        let (cl_type_to_parse, optional_status, trimmed_value) = match cl_type {
            CLType::Option(inner_type) => {
                if value == "null" {
                    (*inner_type, OptionalStatus::None, "")
                } else {
                    (*inner_type, OptionalStatus::Some, value.trim_matches('\''))
                }
            }
            _ => (
                cl_type,
                OptionalStatus::NotOptional,
                value.trim_matches('\''),
            ),
        };

        if value == trimmed_value {
            panic!(
                "value in simple arg should be surrounded by single quotes unless it's a null \
                   optional value"
            );
        }

        match cl_type_to_parse {
            CLType::Bool => {
                let parse = || match trimmed_value.to_lowercase().as_str() {
                    "true" | "t" => true,
                    "false" | "f" => false,
                    invalid => panic!(
                        "can't parse {} as a bool.  Should be 'true' or 'false'",
                        invalid
                    ),
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::I32 => {
                let parse = || {
                    i32::from_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as i32: {}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::I64 => {
                let parse = || {
                    i64::from_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as i64: {}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::U8 => {
                let parse = || {
                    u8::from_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as u8: {}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::U32 => {
                let parse = || {
                    u32::from_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as u32: {}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::U64 => {
                let parse = || {
                    u64::from_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as u64: {}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::U128 => {
                let parse = || {
                    U128::from_dec_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as U128: {}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::U256 => {
                let parse = || {
                    U256::from_dec_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as U256: {}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::U512 => {
                let parse = || {
                    U512::from_dec_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as U512: {}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::Unit => {
                let parse = || {
                    if !trimmed_value.is_empty() {
                        panic!("can't parse {} as unit.  Should be ''", trimmed_value)
                    }
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::String => {
                let parse = || trimmed_value.to_string();
                parse_to_cl_value(optional_status, parse)
            }
            CLType::Key => {
                let parse = || {
                    Key::from_formatted_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as Key: {:?}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::FixedList(ty, 32) => match *ty {
                CLType::U8 => {
                    let parse = || {
                        AccountHash::from_formatted_str(trimmed_value).unwrap_or_else(|error| {
                            panic!("can't parse {} as AccountHash: {:?}", trimmed_value, error)
                        })
                    };
                    parse_to_cl_value(optional_status, parse)
                }
                _ => unreachable!(),
            },
            CLType::URef => {
                let parse = || {
                    URef::from_formatted_str(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as URef: {:?}", trimmed_value, error)
                    })
                };
                parse_to_cl_value(optional_status, parse)
            }
            CLType::PublicKey => {
                let parse = || {
                    let pub_key = NodePublicKey::from_hex(trimmed_value).unwrap_or_else(|error| {
                        panic!("can't parse {} as PublicKey: {:?}", trimmed_value, error)
                    });
                    PublicKey::from(pub_key)
                };
                parse_to_cl_value(optional_status, parse)
            }
            _ => unreachable!(),
        }
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

    const ARG_VALUE_NAME: &str = common::ARG_PATH;
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
mod payment_path {
    use super::*;

    pub(in crate::deploy) const ARG_NAME: &str = "payment-path";
    const ARG_VALUE_NAME: &str = common::ARG_PATH;
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
mod standard_payment {
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

pub(super) fn parse_session_info(matches: &ArgMatches) -> ExecutableDeployItem {
    let session_args = args_from_simple_or_complex(
        arg_simple::session::get(matches),
        args_complex::session::get(matches),
    );

    if let Some(name) = session_name::get(matches) {
        return ExecutableDeployItem::StoredContractByName {
            name,
            args: session_args.to_bytes().expect("should serialize"),
            entry_point: require_session_entry_point(matches),
        };
    }

    if let Some(hash) = session_hash::get(matches) {
        return ExecutableDeployItem::StoredContractByHash {
            hash,
            args: session_args.to_bytes().expect("should serialize"),
            entry_point: require_session_entry_point(matches),
        };
    }

    let version = session_version::get(matches);
    if let Some(name) = session_package_name::get(matches) {
        return ExecutableDeployItem::StoredVersionedContractByName {
            name,
            version, // defaults to highest enabled version
            args: session_args.to_bytes().expect("should serialize"),
            entry_point: require_session_entry_point(matches),
        };
    }

    if let Some(hash) = session_package_hash::get(matches) {
        return ExecutableDeployItem::StoredVersionedContractByHash {
            hash,
            version, // defaults to highest enabled version
            args: session_args.to_bytes().expect("should serialize"),
            entry_point: require_session_entry_point(matches),
        };
    }

    if let Some(module_bytes) = session_path::get(matches) {
        return ExecutableDeployItem::ModuleBytes {
            module_bytes,
            args: session_args.to_bytes().expect("should serialize"),
        };
    }

    panic!(
        "Expected at least one of --{}, --{}, --{}, --{} or --{}",
        session_name::ARG_NAME,
        session_hash::ARG_NAME,
        session_package_name::ARG_NAME,
        session_package_hash::ARG_NAME,
        session_path::ARG_NAME,
    );
}

fn parse_payment_info(matches: &ArgMatches) -> ExecutableDeployItem {
    if let Some(payment_args) = standard_payment::get(matches) {
        return ExecutableDeployItem::ModuleBytes {
            module_bytes: vec![],
            args: payment_args.to_bytes().expect("should serialize"),
        };
    }

    let payment_args = args_from_simple_or_complex(
        arg_simple::payment::get(matches),
        args_complex::payment::get(matches),
    );

    if let Some(name) = payment_name::get(matches) {
        return ExecutableDeployItem::StoredContractByName {
            name,
            args: payment_args.to_bytes().expect("should serialize"),
            entry_point: require_payment_entry_point(matches),
        };
    }

    if let Some(hash) = payment_hash::get(matches) {
        return ExecutableDeployItem::StoredContractByHash {
            hash,
            args: payment_args.to_bytes().expect("should serialize"),
            entry_point: require_payment_entry_point(matches),
        };
    }

    let version = payment_version::get(matches);
    if let Some(name) = payment_package_name::get(matches) {
        return ExecutableDeployItem::StoredVersionedContractByName {
            name,
            version, // defaults to highest enabled version
            args: payment_args.to_bytes().expect("should serialize"),
            entry_point: require_payment_entry_point(matches),
        };
    }

    if let Some(hash) = payment_package_hash::get(matches) {
        return ExecutableDeployItem::StoredVersionedContractByHash {
            hash,
            version, // defaults to highest enabled version
            args: payment_args.to_bytes().expect("should serialize"),
            entry_point: require_payment_entry_point(matches),
        };
    }

    if let Some(module_bytes) = payment_path::get(matches) {
        return ExecutableDeployItem::ModuleBytes {
            module_bytes,
            args: payment_args.to_bytes().expect("should serialize"),
        };
    }

    panic!(
        "Expected at least one of --{}, --{}, --{}, --{}, --{} or --{}",
        standard_payment::ARG_NAME,
        payment_name::ARG_NAME,
        payment_hash::ARG_NAME,
        payment_package_name::ARG_NAME,
        payment_package_hash::ARG_NAME,
        payment_path::ARG_NAME,
    );
}

fn require_session_entry_point(matches: &ArgMatches) -> String {
    session_entry_point::get(matches)
        .unwrap_or_else(|| panic!("{} must be present", session_entry_point::ARG_NAME,))
}

fn require_payment_entry_point(matches: &ArgMatches) -> String {
    payment_entry_point::get(matches)
        .unwrap_or_else(|| panic!("{} must be present", payment_entry_point::ARG_NAME,))
}

pub(super) fn apply_common_creation_options<'a, 'b>(
    subcommand: App<'a, 'b>,
    include_node_address: bool,
) -> App<'a, 'b> {
    let mut subcommand = subcommand
        .setting(AppSettings::NextLineHelp)
        .arg(show_arg_examples::arg());

    if include_node_address {
        subcommand = subcommand.arg(
            common::node_address::arg(DisplayOrder::NodeAddress as usize)
                .required_unless(show_arg_examples::ARG_NAME),
        );
    }

    subcommand = subcommand
        .arg(
            common::secret_key::arg(DisplayOrder::SecretKey as usize)
                .required_unless(show_arg_examples::ARG_NAME),
        )
        .arg(timestamp::arg())
        .arg(ttl::arg())
        .arg(gas_price::arg())
        .arg(dependencies::arg())
        .arg(chain_name::arg());
    subcommand
}

pub(super) fn apply_common_session_options<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
    subcommand
        .arg(session_path::arg())
        .arg(session_package_hash::arg())
        .arg(session_package_name::arg())
        .arg(session_hash::arg())
        .arg(session_name::arg())
        .arg(arg_simple::session::arg())
        .arg(args_complex::session::arg())
        // Group the session-arg args so only one style is used to ensure consistent ordering.
        .group(
            ArgGroup::with_name("session-args")
                .arg(arg_simple::session::ARG_NAME)
                .arg(args_complex::session::ARG_NAME)
                .required(false),
        )
        .arg(session_entry_point::arg())
        .arg(session_version::arg())
        .group(
            ArgGroup::with_name("session")
                .arg(session_path::ARG_NAME)
                .arg(session_package_hash::ARG_NAME)
                .arg(session_package_name::ARG_NAME)
                .arg(session_hash::ARG_NAME)
                .arg(session_name::ARG_NAME)
                .arg(show_arg_examples::ARG_NAME)
                .required(true),
        )
}

pub(crate) fn apply_common_payment_options(
    subcommand: App<'static, 'static>,
) -> App<'static, 'static> {
    subcommand
        .arg(standard_payment::arg())
        .arg(payment_path::arg())
        .arg(payment_package_hash::arg())
        .arg(payment_package_name::arg())
        .arg(payment_hash::arg())
        .arg(payment_name::arg())
        .arg(arg_simple::payment::arg())
        .arg(args_complex::payment::arg())
        // Group the payment-arg args so only one style is used to ensure consistent ordering.
        .group(
            ArgGroup::with_name("payment-args")
                .arg(arg_simple::payment::ARG_NAME)
                .arg(args_complex::payment::ARG_NAME)
                .required(false),
        )
        .arg(payment_entry_point::arg())
        .arg(payment_version::arg())
        .group(
            ArgGroup::with_name("payment")
                .arg(standard_payment::ARG_NAME)
                .arg(payment_path::ARG_NAME)
                .arg(payment_package_hash::ARG_NAME)
                .arg(payment_package_name::ARG_NAME)
                .arg(payment_hash::ARG_NAME)
                .arg(payment_name::ARG_NAME)
                .arg(show_arg_examples::ARG_NAME)
                .required(true),
        )
}

pub(super) fn show_arg_examples_and_exit_if_required(matches: &ArgMatches<'_>) {
    // If we printed the arg examples, exit the process.
    if show_arg_examples::get(matches) {
        process::exit(0);
    }
}

pub(super) fn parse_deploy(matches: &ArgMatches<'_>, session: ExecutableDeployItem) -> Deploy {
    let secret_key = common::secret_key::get(matches);
    let timestamp = timestamp::get(matches);
    let ttl = ttl::get(matches);
    let gas_price = gas_price::get(matches);
    let dependencies = dependencies::get(matches);
    let chain_name = chain_name::get(matches);

    let mut rng = rand::thread_rng();

    let payment = parse_payment_info(matches);

    Deploy::new(
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        payment,
        session,
        &secret_key,
        &mut rng,
    )
}

pub(super) fn construct_deploy(
    matches: &ArgMatches<'_>,
    session: ExecutableDeployItem,
) -> PutDeployParams {
    let deploy = parse_deploy(matches, session);
    PutDeployParams { deploy }
}

pub(super) mod output {
    use super::*;

    const ARG_NAME: &str = "output";
    const ARG_SHORT_NAME: &str = "o";
    const ARG_VALUE_NAME: &str = common::ARG_PATH;
    const ARG_HELP: &str = "Path to output deploy file. If omitted, defaults to stdout. If file exists, it will be overwritten";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .required(false)
            .long(ARG_NAME)
            .short(ARG_SHORT_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Output as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(|v| v.to_string())
    }

    /// Creates a Write trait object for File or Stdout respective to the path value passed
    /// Stdout is used when None
    pub fn output_or_stdout(maybe_path: &Option<String>) -> io::Result<Box<dyn Write>> {
        match maybe_path {
            Some(output_path) => File::create(&output_path).map(|f| Box::new(f) as Box<dyn Write>),
            None => Ok(Box::new(std::io::stdout()) as Box<dyn Write>),
        }
    }

    /// Write the deploy to a file, or if maybe_path is None, stdout
    pub fn write_deploy(deploy: &Deploy, maybe_path: Option<String>) {
        let target = maybe_path.clone().unwrap_or_else(|| "stdout".to_string());
        let mut out = output_or_stdout(&maybe_path)
            .unwrap_or_else(|error| panic!("unable to open {} : {:?}", target, error));
        let content = serde_json::to_string_pretty(deploy)
            .unwrap_or_else(|error| panic!("failed to encode deploy to json: {}", error));
        match out.write_all(content.as_bytes()) {
            Ok(_) if target == "stdout" => {}
            Ok(_) => println!("Successfully wrote deploy to file {}", target),
            Err(err) => panic!("error writing deploy to {}: {}", target, err),
        }
    }
}

pub(super) mod input {
    use super::*;

    const ARG_NAME: &str = "input";
    const ARG_SHORT_NAME: &str = "i";
    const ARG_VALUE_NAME: &str = common::ARG_PATH;
    const ARG_HELP: &str = "Path to input deploy file";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .required_unless(show_arg_examples::ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Input as usize)
    }

    pub fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }

    /// Read a deploy from a file
    pub fn read_deploy(input_path: &str) -> Deploy {
        let input = common::read_file(&input_path);
        let input = String::from_utf8(input).unwrap_or_else(|error| {
            panic!(
                "failed to parse as utf-8 for deploy file {}: {}",
                input_path, error
            )
        });
        serde_json::from_str(input.as_str()).unwrap_or_else(|error| {
            panic!(
                "failed to decode from json for deploy file {}: {}",
                input_path, error
            )
        })
    }
}

fn get_contract_hash(name: &str, matches: &ArgMatches) -> Option<ContractHash> {
    if let Some(value) = matches.value_of(name) {
        if let Ok(digest) = Digest::from_hex(value) {
            return Some(digest.to_array());
        }
        if let Ok(Key::Hash(hash)) = Key::from_formatted_str(value) {
            return Some(hash);
        }
        panic!("Failed to parse {} as a contract hash", value);
    }
    None
}

mod session_hash {
    use super::*;

    pub const ARG_NAME: &str = "session-hash";
    const ARG_VALUE_NAME: &str = common::ARG_HEX_STRING;
    const ARG_HELP: &str = "Hex-encoded hash of the stored contract to be called as the session";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .requires(session_entry_point::ARG_NAME)
            .display_order(DisplayOrder::SessionHash as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<ContractHash> {
        get_contract_hash(ARG_NAME, matches)
    }
}

mod session_name {
    use super::*;

    pub const ARG_NAME: &str = "session-name";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_HELP: &str = "Name of the stored contract (associated with the executing account) to be called as the session";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .requires(session_entry_point::ARG_NAME)
            .display_order(DisplayOrder::SessionName as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(str::to_string)
    }
}

mod session_package_hash {
    use super::*;

    pub const ARG_NAME: &str = "session-package-hash";
    const ARG_VALUE_NAME: &str = common::ARG_HEX_STRING;
    const ARG_HELP: &str = "Hex-encoded hash of the stored package to be called as the session";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .requires(session_entry_point::ARG_NAME)
            .display_order(DisplayOrder::SessionPackageHash as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<ContractHash> {
        get_contract_hash(ARG_NAME, matches)
    }
}

mod session_package_name {
    use super::*;

    pub const ARG_NAME: &str = "session-package-name";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_HELP: &str = "Name of the stored package to be called as the session";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .requires(session_entry_point::ARG_NAME)
            .display_order(DisplayOrder::SessionPackageName as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(str::to_string)
    }
}

mod session_entry_point {
    use super::*;

    pub const ARG_NAME: &str = "session-entry-point";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_HELP: &str = "Name of the method that will be used when calling the session contract";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .display_order(DisplayOrder::SessionEntryPoint as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(str::to_string)
    }
}

mod session_version {
    use super::*;

    pub const ARG_NAME: &str = "session-version";
    const ARG_VALUE_NAME: &str = common::ARG_INTEGER;
    const ARG_HELP: &str = "Version of the called session contract. Latest will be used by default";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .display_order(DisplayOrder::SessionVersion as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<u32> {
        matches
            .value_of(ARG_NAME)
            .map(|s| s.parse::<u32>().ok())
            .flatten()
    }
}

mod payment_hash {
    use super::*;

    pub const ARG_NAME: &str = "payment-hash";
    const ARG_VALUE_NAME: &str = common::ARG_HEX_STRING;
    const ARG_HELP: &str = "Hex-encoded hash of the stored contract to be called as the payment";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .requires(payment_entry_point::ARG_NAME)
            .display_order(DisplayOrder::PaymentHash as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<ContractHash> {
        get_contract_hash(ARG_NAME, matches)
    }
}

mod payment_name {
    use super::*;

    pub const ARG_NAME: &str = "payment-name";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_HELP: &str = "Name of the stored contract (associated with the executing account) \
    to be called as the payment";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .requires(payment_entry_point::ARG_NAME)
            .display_order(DisplayOrder::PaymentName as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(str::to_string)
    }
}

mod payment_package_hash {
    use super::*;

    pub const ARG_NAME: &str = "payment-package-hash";
    const ARG_VALUE_NAME: &str = common::ARG_HEX_STRING;
    const ARG_HELP: &str = "Hex-encoded hash of the stored package to be called as the payment";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .requires(payment_entry_point::ARG_NAME)
            .display_order(DisplayOrder::PaymentPackageHash as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<ContractHash> {
        get_contract_hash(ARG_NAME, matches)
    }
}

mod payment_package_name {
    use super::*;

    pub const ARG_NAME: &str = "payment-package-name";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_HELP: &str = "Name of the stored package to be called as the payment";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .requires(payment_entry_point::ARG_NAME)
            .display_order(DisplayOrder::PaymentPackageName as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(str::to_string)
    }
}

mod payment_entry_point {
    use super::*;

    pub const ARG_NAME: &str = "payment-entry-point";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_HELP: &str = "Name of the method that will be used when calling the payment contract";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .display_order(DisplayOrder::PaymentEntryPoint as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(str::to_string)
    }
}

mod payment_version {
    use super::*;

    pub const ARG_NAME: &str = "payment-version";
    const ARG_VALUE_NAME: &str = common::ARG_INTEGER;
    const ARG_HELP: &str = "Version of the called payment contract. Latest will be used by default";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .required(false)
            .display_order(DisplayOrder::PaymentVersion as usize)
    }

    pub fn get(matches: &ArgMatches) -> Option<u32> {
        matches
            .value_of(ARG_NAME)
            .map(|s| s.parse::<u32>().ok())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_simple_args_test<T: CLTyped + ToBytes>(cli_string: &str, expected: T) {
        let matches = App::new("app")
            .arg(arg_simple::payment::arg())
            .arg(arg_simple::session::arg())
            .get_matches_from(vec![
                "app",
                &format!("--{}", arg_simple::payment::ARG_NAME),
                cli_string,
                &format!("--{}", arg_simple::session::ARG_NAME),
                cli_string,
            ]);

        let expected = Some(RuntimeArgs::from(vec![NamedArg::new(
            "x".to_string(),
            CLValue::from_t(expected).unwrap(),
        )]));

        assert_eq!(arg_simple::payment::get(&matches), expected);
        assert_eq!(arg_simple::session::get(&matches), expected);
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
