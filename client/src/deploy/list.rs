use std::str;

use clap::{App, ArgMatches, SubCommand};

use casper_client::ListDeploysResult;
use casper_node::rpcs::chain::GetBlockResult;

use crate::{command::ClientCommand, common, deploy::Error as DeployError};

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
        let mut verbosity_level = common::verbose::get(matches);
        let maybe_block_id = common::block_identifier::get(matches);

        match casper_client::get_block(maybe_rpc_id, node_address, verbosity_level, maybe_block_id)
        {
            Ok(response_value) => {
                let response_value = response_value.get_result().cloned().unwrap();
                let get_block_result =
                    serde_json::from_value::<GetBlockResult>(response_value).expect("should parse");
                let list_deploys_result: ListDeploysResult = get_block_result.into();

                if verbosity_level == 0 {
                    verbosity_level += 1
                }
                casper_client::pretty_print_at_level(&list_deploys_result, verbosity_level);
            }
            Err(error) => println!("{}", DeployError::from(error)),
        }
    }
}
