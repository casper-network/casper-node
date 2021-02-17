use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::chain::GetStateRootHash;

use crate::{block::Error as BlockError, command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    BlockHash,
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetStateRootHash {
    const NAME: &'static str = "get-state-root-hash";
    const ABOUT: &'static str = "Retrieves a state root hash at a given block";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(common::block_identifier::arg(
                DisplayOrder::BlockHash as usize,
            ))
    }

    fn run(matches: &ArgMatches<'_>) {
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let mut verbosity_level = common::verbose::get(matches);
        let maybe_block_id = common::block_identifier::get(matches);

        let response = casper_client::get_state_root_hash(
            maybe_rpc_id,
            node_address,
            verbosity_level,
            maybe_block_id,
        );

        if verbosity_level == 0 {
            verbosity_level += 1
        }

        match response {
            Ok(response) => casper_client::pretty_print_at_level(&response, verbosity_level),
            Err(error) => println!("{}", BlockError::from(error)),
        }
    }
}
