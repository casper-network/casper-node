use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};
use serde_json::{json, Map, Value};

use crate::{command::ClientCommand, common, rpc::RpcClient};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    GlobalStateHash,
    PurseURef,
}

pub struct GetBalance {}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BalanceArgs {
    pub global_state_hash: String,
    pub purse_uref: String,
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

    pub(super) fn get(matches: &ArgMatches) -> Map<String, Value> {
        let purse_uref = purse_uref::get(&matches);
        let global_state_hash = common::global_state_hash::get(&matches);

        json!(BalanceArgs {
            global_state_hash,
            purse_uref,
        })
        .as_object()
        .unwrap()
        .clone()
    }
}

impl RpcClient for GetBalance {
    const RPC_METHOD: &'static str = "state_get_balance";
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
            .arg(common::global_state_hash::arg(
                DisplayOrder::GlobalStateHash as usize,
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
