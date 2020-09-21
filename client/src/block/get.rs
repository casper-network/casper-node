use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_node::rpcs::{
    info::{GetBlock, GetBlockParams},
    RpcWithParams,
};

use crate::{command::ClientCommand, common, RpcClient};

/// This struct defines the order in which the args are shown for this subcommand
enum DisplayOrder {
    NodeAddress,
    BlockHash,
}

/// Handles providing the arg for and retrieval of the deploy hash
mod block_hash {
    use super::*;

    const ARG_NAME: &str = "block-hash";
    const ARG_VALUE_NAME: &str =  "HEX STRING";
    const ARG_HELP: &str = "Hex-encoded block hash";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::BlockHash as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

impl RpcClient for GetBlock {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetBlock {
    const NAME: &'static str = "get-block";
    const ABOUT: &'static  str = "Retrieves a block";

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
        let block_hash = block_hash::get(matches);
        let params = GetBlockParams { block_hash };

        let response_value = Self::request_with_map_params(&node_address, params)
            .unwrap_or_else(|error| panic!("response error {}", error));
        println!("{}", response_value);
    }
}