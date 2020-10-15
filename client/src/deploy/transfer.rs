use clap::{App, Arg, ArgGroup, ArgMatches, SubCommand};

use casper_node::crypto::asymmetric_key::PublicKey;
use casper_types::{URef, U512};

use super::creation_common::{self, DisplayOrder};
use crate::{command::ClientCommand, common};

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
            .required_unless(creation_common::show_arg_examples::ARG_NAME)
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

    // Conflicts with --target-purse, but that's handled via an `ArgGroup` in the subcommand.  Don't
    // add a `conflicts_with()` to the arg or the `ArgGroup` fails to work correctly.
    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
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

    // Conflicts with --target-account, but that's handled via an `ArgGroup` in the subcommand.
    // Don't add a `conflicts_with()` to the arg or the `ArgGroup` fails to work correctly.
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

pub struct Transfer {}

impl<'a, 'b> ClientCommand<'a, 'b> for Transfer {
    const NAME: &'static str = "transfer";
    const ABOUT: &'static str = "Transfers funds between purses";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(amount::arg())
            .arg(source_purse::arg())
            .arg(target_account::arg())
            .arg(target_purse::arg())
            // Group the target args to ensure exactly one is required.
            .group(
                ArgGroup::with_name("required-target-args")
                    .arg(target_account::ARG_NAME)
                    .arg(target_purse::ARG_NAME)
                    .arg(creation_common::show_arg_examples::ARG_NAME)
                    .required(true),
            );
        let subcommand = creation_common::apply_common_payment_options(subcommand);
        creation_common::apply_common_creation_options(subcommand, true)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);
        let verbose = common::verbose::get(matches);
        let rpc_id = common::rpc_id::get(matches);
        let deploy_params = creation_common::parse_deploy_params(matches);
        let payment = creation_common::parse_payment_info(matches);

        let response = casper_client::RpcCall::new(rpc_id, verbose).transfer(
            common::node_address::get(matches),
            amount::get(matches),
            source_purse::get(matches),
            target_account::get(matches),
            target_purse::get(matches),
            deploy_params,
            payment,
        )
        .unwrap_or_else(|error| panic!("response error: {}", error));
        println!(
            "{}",
            serde_json::to_string_pretty(&response).expect("should encode to JSON")
        );
    }
}
