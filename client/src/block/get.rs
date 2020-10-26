use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::{RpcWithOptionalParams, chain::{GetBlock, GetBlockParams}};

use crate::{command::ClientCommand, common, RpcClient};

/// This struct defines the order in which the args are shown for this subcommand.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    BlockParameter,
}

impl RpcClient for GetBlock {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetBlock {
    const NAME: &'static str = "get-block";
    const ABOUT: &'static str = "Retrieves a block";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(common::block_parameter::arg(DisplayOrder::BlockParameter as usize))
            
    }

    fn run(matches: &ArgMatches<'_>) {
        let verbose = common::verbose::get(matches);
        let node_address = common::node_address::get(matches);
        let rpc_id = common::rpc_id::get(matches);
        let maybe_block_parameter = common::block_parameter::get(matches);

        let response = match maybe_block_parameter {
            Some(block_parameter) => {
                let params = GetBlockParams { block_parameter };
                Self::request_with_map_params(verbose, &node_address, rpc_id, params)
            }
            None => Self::request(verbose, &node_address, rpc_id),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&response).expect("should encode to JSON")
        );
    }
}
