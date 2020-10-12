use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_node::rpcs::{
    state::{GetBalance, GetBalanceParams},
    RpcWithParams,
};

use crate::{command::ClientCommand, common, rpc::RpcClient};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
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

    pub(super) fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

mod balance_args {
    use super::*;

    pub(super) fn get(matches: &ArgMatches) -> <GetBalance as RpcWithParams>::RequestParams {
        let state_hash = common::state_root_hash::get(&matches);
        let purse_uref = purse_uref::get(&matches);

        GetBalanceParams {
            state_root_hash: state_hash,
            purse_uref,
        }
    }
}

impl RpcClient for GetBalance {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetBalance {
    const NAME: &'static str = "get-balance";
    const ABOUT: &'static str = "Retrieves a stored balance";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::state_root_hash::arg(
                DisplayOrder::StateRootHash as usize,
            ))
            .arg(purse_uref::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let args = balance_args::get(matches);
        let res = Self::request_with_map_params(&node_address, args)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", res);
    }
}
