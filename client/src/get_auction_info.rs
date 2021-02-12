use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::state::GetAuctionInfo;

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetAuctionInfo {
    const NAME: &'static str = "get-auction-info";
    const ABOUT: &'static str =
        "Retrieves the bids and validators as of the most recently added block";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
    }

    fn run(matches: &ArgMatches<'_>) {
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let mut verbosity_level = common::verbose::get(matches);

        let response = casper_client::get_auction_info(maybe_rpc_id, node_address, verbosity_level)
            .unwrap_or_else(|error| panic!("response error: {}", error));

        if verbosity_level == 0 {
            verbosity_level += 1
        }
        casper_client::pretty_print_at_level(&response, verbosity_level);
    }
}
