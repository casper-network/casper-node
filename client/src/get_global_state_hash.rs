use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_client::RpcCall;
use casper_node::rpcs::chain::GetGlobalStateHash;

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    BlockHash,
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetGlobalStateHash {
    const NAME: &'static str = "get-global-state-hash";
    const ABOUT: &'static str = "Retrieves a global state hash";

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
        let rpc = RpcCall::new(
            common::rpc_id::get(matches),
            common::node_address::get(matches),
            common::verbose::get(matches),
        );

        let maybe_block_hash = common::block_hash::get(matches);

        let response = rpc
            .get_global_state_hash(maybe_block_hash)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!(
            "{}",
            serde_json::to_string_pretty(&response).expect("should encode to JSON")
        );
    }
}
