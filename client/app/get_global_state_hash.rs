use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::chain::GetGlobalStateHash;

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    BlockHash,
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
            .arg(common::block_hash::arg(DisplayOrder::BlockHash as usize))
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let maybe_block_hash = common::block_hash::get(matches);
        let response_value = casper_client::get_global_state_hash(node_address, maybe_block_hash)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
