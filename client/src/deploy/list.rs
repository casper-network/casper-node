use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_client::ListDeploysResult;
use casper_node::rpcs::chain::GetBlockResult;

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    BlockHash,
}

pub struct ListDeploys;

impl<'a, 'b> ClientCommand<'a, 'b> for ListDeploys {
    const NAME: &'static str = "list-deploys";
    const ABOUT: &'static str = "Retrieves the list of all deploy hashes in a given block";

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
        let verbose = common::verbose::get(matches);
        let maybe_block_id = common::block_identifier::get(matches);

        let response_value =
            casper_client::get_block(maybe_rpc_id, node_address, verbose, maybe_block_id)
                .unwrap_or_else(|error| panic!("should parse as a GetBlockResult: {}", error));
        let response_value = response_value.get_result().cloned().unwrap();
        let get_block_result =
            serde_json::from_value::<GetBlockResult>(response_value).expect("should parse");
        let list_deploys_result: ListDeploysResult = get_block_result.into();
        println!(
            "{}",
            serde_json::to_string_pretty(&list_deploys_result).expect("should encode to JSON")
        );
    }
}
