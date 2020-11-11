use clap::{App, Arg, ArgGroup, ArgMatches, SubCommand};

use casper_client::DeployStrParams;

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

    pub(in crate::deploy) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
    }
}

/// Handles providing the arg for and retrieval of the source purse.
mod source_purse {
    use super::*;

    pub(super) const ARG_NAME: &str = "source-purse";
    const ARG_VALUE_NAME: &str = "UREF";
    const ARG_HELP: &str =
        "URef of the source purse. If this is omitted, the main purse of the account creating this \
        transfer will be used as the source purse";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferSourcePurse as usize)
    }

    pub(super) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches.value_of(ARG_NAME).unwrap_or_default()
    }
}

/// Handles providing the arg for and retrieval of the target account.
pub(super) mod target_account {
    use super::*;

    pub(super) const ARG_NAME: &str = "target-account";
    const ARG_SHORT: &str = "t";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str =
        "Hex-encoded public key of the account from which the main purse will be used as the \
        target.";

    // Conflicts with --target-purse, but that's handled via an `ArgGroup` in the subcommand. Don't
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

    pub(super) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches.value_of(ARG_NAME).unwrap_or_default()
    }
}

/// Handles providing the arg for and retrieval of the target purse.
pub(super) mod target_purse {
    use super::*;

    pub(super) const ARG_NAME: &str = "target-purse";
    const ARG_VALUE_NAME: &str = "UREF";
    const ARG_HELP: &str = "URef of the target purse";

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

    pub(super) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches.value_of(ARG_NAME).unwrap_or_default()
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

        let amount = amount::get(matches);
        let source_purse = source_purse::get(matches);
        let target_purse = target_purse::get(matches);
        let target_account = target_account::get(matches);

        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let verbose = common::verbose::get(matches);

        let secret_key = common::secret_key::get(matches);
        let timestamp = creation_common::timestamp::get(matches);
        let ttl = creation_common::ttl::get(matches);
        let gas_price = creation_common::gas_price::get(matches);
        let dependencies = creation_common::dependencies::get(matches);
        let chain_name = creation_common::chain_name::get(matches);

        let payment_str_params = creation_common::payment_str_params(matches);

        let response = casper_client::transfer(
            maybe_rpc_id,
            node_address,
            verbose,
            amount,
            source_purse,
            target_purse,
            target_account,
            DeployStrParams {
                secret_key,
                timestamp,
                ttl,
                dependencies,
                gas_price,
                chain_name,
            },
            payment_str_params,
        )
        .unwrap_or_else(|err| panic!("unable to put deploy {:?}", err));
        println!(
            "{}",
            serde_json::to_string_pretty(&response).expect("should encode to JSON")
        );
    }
}
