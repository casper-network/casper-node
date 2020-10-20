use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_node::rpcs::state::GetBalance;
use casper_types::URef;

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    StateRootHash,
    PurseURef,
}

/// Handles providing the arg for and retrieval of the purse URef.
mod purse_uref {

use super::*;

    const ARG_NAME: &str = "purse-uref";
    const ARG_SHORT: &str = "p";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING";
    const ARG_HELP: &str =
        "The URef under which the purse is stored.  This must be a properly formatted URef \
        \"uref-<HEX STRING>-<THREE DIGIT INTEGER>\"";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::PurseURef as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> URef {
        let value = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));

        URef::from_formatted_str(&value)
            .unwrap_or_else(|error| panic!("can't parse {} as URef: {:?}", value, error))
    }
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetBalance {
    const NAME: &'static str = "get-balance";
    const ABOUT: &'static str = "Retrieves a stored balance";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(common::state_root_hash::arg(
                DisplayOrder::StateRootHash as usize,
            ))
            .arg(purse_uref::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let rpc = common::rpc(matches);

        let state_root_hash = common::state_root_hash::get(&matches);
        let purse_uref = purse_uref::get(&matches);

        let response = rpc
            .get_balance(state_root_hash, purse_uref)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!(
            "{}",
            serde_json::to_string_pretty(&response).expect("should encode to JSON")
        );
    }
}
