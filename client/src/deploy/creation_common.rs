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

use casper_client::{cl_type, DeployParams, ExecutableDeployItemExt};
use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_node::{
    crypto::{asymmetric_key::PublicKey as NodePublicKey, hash::Digest},
    types::{Deploy, TimeDiff, Timestamp},
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
    AccessRights, CLType, CLValue, ContractHash, Key, NamedArg, RuntimeArgs, URef, U512,
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
pub(super) mod gas_price {
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
pub(super) mod dependencies {
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
pub(super) mod chain_name {
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
pub(super) mod session {
    use super::*;

    const ARG_NAME: &str = "session-path";
    const ARG_SHORT: &str = "s";
    const ARG_VALUE_NAME: &str = common::ARG_PATH;
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
        static ref ARG_HELP: String = format!(
            "For simple CLTypes, a named and typed arg which is passed to the Wasm code. To see \
            an example for each type, run '--{}'. This arg can be repeated to pass multiple named, \
            typed args, but can only be used for the following types: {}",
            super::show_arg_examples::ARG_NAME,
            cl_type::supported_cl_type_list()
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
pub(super) mod payment {
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

pub(super) fn parse_deploy_params(matches: &ArgMatches<'_>) -> DeployParams {
    let secret_key = common::secret_key::get(matches);
    let timestamp = timestamp::get(matches);
    let ttl = ttl::get(matches);
    let gas_price = gas_price::get(matches);
    let dependencies = dependencies::get(matches);
    let chain_name = chain_name::get(matches);

    DeployParams {
        timestamp,
        ttl,
        gas_price,
        dependencies,
        chain_name,
        secret_key,
    }
}

pub(super) fn parse_session_module_args(matches: &ArgMatches<'_>) -> (Vec<u8>, RuntimeArgs) {
    let module_bytes = session::get(matches);
    let session_args = args_from_simple_or_complex(
        arg_simple::session::get(matches),
        args_complex::session::get(matches),
    );
    (module_bytes, session_args)
}

pub(super) fn parse_session_info(matches: &ArgMatches) -> ExecutableDeployItem {
    let (module_bytes, session_args) = parse_session_module_args(matches);
    if let Some(name) = session_name::get(matches) {
        return ExecutableDeployItem::new_stored_contract_by_name(
            name,
            require_session_entry_point(matches),
            session_args,
        )
        .expect("should serialize");
    }
    if let Some(hash) = session_hash::get(matches) {
        return ExecutableDeployItem::new_stored_contract_by_hash(
            hash,
            require_session_entry_point(matches),
            session_args,
        )
        .expect("should serialize");
    }
    if let Some(version) = session_version::get(matches) {
        if let Some(name) = session_package_name::get(matches) {
            return ExecutableDeployItem::new_stored_versioned_contract_by_name(
                name,
                version,
                require_session_entry_point(matches),
                session_args,
            )
            .expect("should serialize");
        }
        if let Some(hash) = session_package_hash::get(matches) {
            return ExecutableDeployItem::new_stored_versioned_contract_by_hash(
                hash,
                version,
                require_session_entry_point(matches),
                session_args,
            )
            .expect("should serialize");
        }
    }
    ExecutableDeployItem::ModuleBytes {
        module_bytes,
        args: session_args.to_bytes().expect("should serialize"),
    }
}

pub(super) fn parse_payment_info(matches: &ArgMatches) -> ExecutableDeployItem {
    let (module_bytes, payment_args) = parse_payment_module_args(matches);
    if let Some(name) = payment_name::get(matches) {
        return ExecutableDeployItem::new_stored_contract_by_name(
            name,
            require_payment_entry_point(matches),
            payment_args,
        )
        .expect("should serialize");
    }
    if let Some(hash) = payment_hash::get(matches) {
        return ExecutableDeployItem::new_stored_contract_by_hash(
            hash,
            require_payment_entry_point(matches),
            payment_args,
        )
        .expect("should serialize");
    }
    if let Some(version) = payment_version::get(matches) {
        if let Some(name) = payment_package_name::get(matches) {
            return ExecutableDeployItem::new_stored_versioned_contract_by_name(
                name,
                version,
                require_payment_entry_point(matches),
                payment_args,
            )
            .expect("should serialize");
        }
        if let Some(hash) = payment_package_hash::get(matches) {
            return ExecutableDeployItem::new_stored_versioned_contract_by_hash(
                hash,
                version,
                require_payment_entry_point(matches),
                payment_args,
            )
            .expect("should serialize");
        }
    }
    ExecutableDeployItem::ModuleBytes {
        module_bytes,
        args: payment_args.to_bytes().expect("should serialize"),
    }
}

fn require_session_entry_point(matches: &ArgMatches) -> String {
    session_entry_point::get(matches)
        .unwrap_or_else(|| panic!("{} must be present", session_entry_point::ARG_NAME,))
}

fn require_payment_entry_point(matches: &ArgMatches) -> String {
    payment_entry_point::get(matches)
        .unwrap_or_else(|| panic!("{} must be present", payment_entry_point::ARG_NAME,))
}

pub(super) fn parse_payment_module_args(matches: &ArgMatches) -> (Vec<u8>, RuntimeArgs) {
    if let Some(payment_args) = standard_payment::get(matches) {
        return (vec![], payment_args);
    }

    // Get the payment code and args options.
    let module_bytes = payment::get(matches).expect("should have payment-amount or payment-path");
    let payment_args = args_from_simple_or_complex(
        arg_simple::payment::get(matches),
        args_complex::payment::get(matches),
    );
    (module_bytes, payment_args)
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
        );
    subcommand
}

pub(super) fn apply_common_session_options<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
    subcommand
        .arg(session::arg())
        .arg(arg_simple::session::arg())
        .arg(args_complex::session::arg())
        // Group the session-arg args so only one style is used to ensure consistent ordering.
        .group(
            ArgGroup::with_name("session-args")
                .arg(arg_simple::session::ARG_NAME)
                .arg(args_complex::session::ARG_NAME)
                .required(false),
        )
        .arg(session_package_hash::arg())
        .arg(session_package_name::arg())
        .arg(session_hash::arg())
        .arg(session_name::arg())
        .arg(session_entry_point::arg())
        .arg(session_version::arg())
}

pub(crate) fn apply_common_payment_options(
    subcommand: App<'static, 'static>,
) -> App<'static, 'static> {
    subcommand
        .arg(payment_package_hash::arg())
        .arg(payment_package_name::arg())
        .arg(payment_hash::arg())
        .arg(payment_name::arg())
        .arg(payment_entry_point::arg())
        .arg(payment_version::arg())
}

pub(super) fn show_arg_examples_and_exit_if_required(matches: &ArgMatches<'_>) {
    // If we printed the arg examples, exit the process.
    if show_arg_examples::get(matches) {
        process::exit(0);
    }
}

pub fn parse_deploy(matches: &ArgMatches<'_>, session: ExecutableDeployItem) -> Deploy {
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

    pub fn get<'a>(matches: &'a ArgMatches) -> Option<&'a str> {
        matches.value_of(ARG_NAME)
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
}

fn get_contract_hash(name: &str, matches: &ArgMatches) -> Option<ContractHash> {
    if let Some(v) = matches.value_of(name) {
        if let Ok(Key::Hash(hash)) = Key::from_formatted_str(v) {
            return Some(hash);
        }
    }
    None
}

pub(crate) mod session_hash {
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

pub(crate) mod session_name {
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

pub(crate) mod session_package_hash {
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

pub(crate) mod session_package_name {
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

pub(crate) mod session_entry_point {
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

pub(crate) mod session_version {
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

pub(crate) mod payment_hash {
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

pub(crate) mod payment_name {
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

pub(crate) mod payment_package_hash {
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

pub(crate) mod payment_package_name {
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

pub(crate) mod payment_entry_point {
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

pub(crate) mod payment_version {
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
