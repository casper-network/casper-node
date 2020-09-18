use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_node::rpcs::{
    chain::{GetGlobalStateHash, GetGlobalStateHashParams},
    RpcWithOptionalParams,
};

use crate::{command::ClientCommand, common, RpcClient};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    BlockHash,
}

/// Handles providing the arg for and retrieval of the block hash.
mod block_hash {
    use super::*;

    const ARG_NAME: &str = "block-hash";
    const ARG_SHORT: &str = "b";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str =
        "Hex-encoded block hash.  If not given, the latest finalized block as known at the given \
        node will be used";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::BlockHash as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(ToString::to_string)
    }
}

impl RpcClient for GetGlobalStateHash {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetGlobalStateHash {
    const NAME: &'static str = "get-global-state-hash";
    const ABOUT: &'static str = "Retrieves a global state hash";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(block_hash::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let maybe_block_hash = block_hash::get(matches);

        let response_value = match maybe_block_hash {
            Some(block_hash) => {
                let params = GetGlobalStateHashParams { block_hash };
                Self::request_with_map_params(&node_address, params)
            }
            None => Self::request(&node_address),
        }
        .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
