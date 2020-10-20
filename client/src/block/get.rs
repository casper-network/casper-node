use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::chain::GetBlock;

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    BlockHash,
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
            .arg(common::block_hash::arg(DisplayOrder::BlockHash as usize))
    }

    fn run(matches: &ArgMatches<'_>) {
        let rpc = common::rpc(matches);

        let maybe_block_hash = common::block_hash::get(matches);
        let response = rpc
            .get_block(maybe_block_hash)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!(
            "{}",
            serde_json::to_string_pretty(&response).expect("should encode to JSON")
        );
    }
}
