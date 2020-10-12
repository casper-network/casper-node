use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::{
    chain::{GetStateRootHash, GetStateRootHashParams},
    RpcWithOptionalParams,
};

use crate::{command::ClientCommand, common, RpcClient};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    BlockHash,
}

impl RpcClient for GetStateRootHash {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetStateRootHash {
    const NAME: &'static str = "get-state-root-hash";
    const ABOUT: &'static str = "Retrieves a hash of the state root";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::block_hash::arg(DisplayOrder::BlockHash as usize))
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let maybe_block_hash = common::block_hash::get(matches);

        let response_value = match maybe_block_hash {
            Some(block_hash) => {
                let params = GetStateRootHashParams { block_hash };
                Self::request_with_map_params(&node_address, params)
            }
            None => Self::request(&node_address),
        }
        .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
