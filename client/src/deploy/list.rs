use std::str;

use clap::{App, ArgMatches, SubCommand};
use semver::Version;
use serde::{Deserialize, Serialize};

use casper_node::{rpcs::chain::GetBlockResult, types::DeployHash};

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    BlockHash,
}

pub struct ListDeploys {}

/// Result for "chain_get_block" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct ListDeploysResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The deploy hashes of the block, if found.
    pub deploy_hashes: Option<Vec<DeployHash>>,
}

impl From<GetBlockResult> for ListDeploysResult {
    fn from(get_block_result: GetBlockResult) -> Self {
        ListDeploysResult {
            api_version: get_block_result.api_version,
            deploy_hashes: get_block_result
                .block
                .map(|block| block.deploy_hashes().clone()),
        }
    }
}

impl<'a, 'b> ClientCommand<'a, 'b> for ListDeploys {
    const NAME: &'static str = "list-deploys";
    const ABOUT: &'static str = "Gets the list of all deploy hashes from a given block";

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

        let response_value = rpc
            .list_deploys(maybe_block_hash)
            .unwrap_or_else(|error| panic!("should parse as a GetBlockResult: {}", error));
        println!(
            "{}",
            serde_json::to_string_pretty(&response_value).expect("should encode to JSON")
        );
    }
}
