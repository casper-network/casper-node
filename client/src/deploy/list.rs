use std::str;

use clap::{App, ArgMatches, SubCommand};
use semver::Version;
use serde::{Deserialize, Serialize};

use casper_node::{
    rpcs::{
        chain::{GetBlock, GetBlockParams, GetBlockResult},
        RpcWithOptionalParams,
    },
    types::DeployHash,
};

use crate::{command::ClientCommand, common, RpcClient};

/// This struct defines the order in which the args are shown for this subcommand.
enum DisplayOrder {
    NodeAddress,
    BlockHash,
}

pub struct ListDeploys {}

impl RpcClient for ListDeploys {
    const RPC_METHOD: &'static str = GetBlock::METHOD;
}

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
                let params = GetBlockParams { block_hash };
                Self::request_with_map_params(&node_address, params)
            }
            None => Self::request(&node_address),
        }
        .unwrap_or_else(|error| panic!("response error: {}", error));

        let get_block_result: GetBlockResult = serde_json::from_value(response_value)
            .unwrap_or_else(|error| panic!("should parse as a GetBlockResult: {}", error));

        let result = ListDeploysResult::from(get_block_result);
        println!("{}", serde_json::to_string(&result).unwrap());
    }
}
