use clap::{App, Arg, ArgGroup, ArgMatches, SubCommand};

use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_node::crypto::asymmetric_key::PublicKey;
use casper_types::{bytesrepr::ToBytes, RuntimeArgs, URef, U512};

use super::creation_common::{self, DisplayOrder};
use crate::{command::ClientCommand, common, RpcClient};

/// Handles providing the arg for and retrieval of the transfer amount.
pub(super) mod amount {
    use super::*;

    const ARG_NAME: &str = "amount";
    const ARG_SHORT: &str = "a";
    const ARG_VALUE_NAME: &str = "512-BIT INTEGER";
    const ARG_HELP: &str = "The number of motes to transfer";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferAmount as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> U512 {
        let value = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));
        U512::from_dec_str(value).unwrap_or_else(|error| {
            panic!("can't parse --{} {} as U512: {}", ARG_NAME, value, error)
        })
    }
}

/// Handles providing the arg for and retrieval of the source purse.
mod source_purse {
    use super::*;

    pub(super) const ARG_NAME: &str = "source-purse";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str =
        "Hex-encoded URef of the source purse. If this is omitted, the main purse of the account \
        creating this transfer will be used as the source purse";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferSourcePurse as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Option<URef> {
        matches.value_of(ARG_NAME).map(|value| {
            URef::from_formatted_str(value).unwrap_or_else(|error| {
                panic!("can't parse --{} {} as URef: {:?}", ARG_NAME, value, error)
            })
        })
    }
}

/// Handles providing the arg for and retrieval of the target account.
mod target_account {
    use super::*;

    pub(super) const ARG_NAME: &str = "target-account";
    const ARG_SHORT: &str = "t";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str =
        "Hex-encoded public key of the account from which the main purse will be used as the \
        target.";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .conflicts_with(super::target_purse::ARG_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferTargetAccount as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Option<PublicKey> {
        matches.value_of(ARG_NAME).map(|value| {
            PublicKey::from_hex(value).unwrap_or_else(|error| {
                panic!(
                    "can't parse --{} {} as PublicKey: {:?}",
                    ARG_NAME, value, error
                )
            })
        })
    }
}

/// Handles providing the arg for and retrieval of the target purse.
mod target_purse {
    use super::*;

    pub(super) const ARG_NAME: &str = "target-purse";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str = "Hex-encoded URef of the target purse";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferTargetPurse as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Option<URef> {
        matches.value_of(ARG_NAME).map(|value| {
            URef::from_formatted_str(value).unwrap_or_else(|error| {
                panic!("can't parse --{} {} as URef: {:?}", ARG_NAME, value, error)
            })
        })
    }
}

fn create_transfer_args(matches: &ArgMatches) -> RuntimeArgs {
    const TRANSFER_ARG_AMOUNT: &str = "amount";
    const TRANSFER_ARG_SOURCE: &str = "source";
    const TRANSFER_ARG_TARGET: &str = "target";

    let mut runtime_args = RuntimeArgs::new();
    runtime_args.insert(TRANSFER_ARG_AMOUNT, amount::get(matches));

    if let Some(source_purse) = source_purse::get(matches) {
        runtime_args.insert(TRANSFER_ARG_SOURCE, source_purse);
    }

    match (target_account::get(matches), target_purse::get(matches)) {
        (Some(target_account), None) => {
            let target_account_hash = target_account.to_account_hash().value();
            runtime_args.insert(TRANSFER_ARG_TARGET, target_account_hash);
        }
        (None, Some(target_purse)) => {
            runtime_args.insert(TRANSFER_ARG_TARGET, target_purse);
        }
        _ => unreachable!("should have a target"),
    }

    runtime_args
}

pub struct Transfer {}

impl RpcClient for Transfer {
    const RPC_METHOD: &'static str = "account_put_deploy";
}

impl<'a, 'b> ClientCommand<'a, 'b> for Transfer {
    const NAME: &'static str = "transfer";
    const ABOUT: &'static str = "Transfers funds between purses";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(amount::arg())
            .arg(source_purse::arg())
            .arg(target_account::arg())
            .arg(target_purse::arg())
            // Group the target args to ensure one is given.
            .group(
                ArgGroup::with_name("target-args")
                    .arg(target_account::ARG_NAME)
                    .arg(target_purse::ARG_NAME)
                    .required(false),
            );
        creation_common::apply_common_creation_options(subcommand)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);

        let node_address = common::node_address::get(matches);

        let transfer_args = create_transfer_args(matches)
            .to_bytes()
            .expect("should serialize");
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };

        let params = creation_common::construct_deploy(matches, session);

        let response_value = Self::request_with_map_params(&node_address, params)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
