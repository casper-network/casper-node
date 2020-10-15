use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_node::{crypto::hash::Digest, rpcs::info::GetDeploy, types::DeployHash};

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    DeployHash,
}

/// Handles providing the arg for and retrieval of the deploy hash.
mod deploy_hash {
    use super::*;

    const ARG_NAME: &str = "deploy-hash";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str = "Hex-encoded deploy hash";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::DeployHash as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> DeployHash {
        let hex_str = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));
        let hash = Digest::from_hex(hex_str)
            .unwrap_or_else(|error| panic!("cannot parse as a deploy hash: {}", error));
        DeployHash::new(hash)
    }
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetDeploy {
    const NAME: &'static str = "get-deploy";
    const ABOUT: &'static str = "Retrieves a stored deploy";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(deploy_hash::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let verbose = common::verbose::get(matches);
        let node_address = common::node_address::get(matches);
        let rpc_id = common::rpc_id::get(matches);
        let deploy_hash = deploy_hash::get(matches);
        let response = casper_client::RpcCall::new(rpc_id, verbose)
            .get_deploy(node_address, deploy_hash)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!(
            "{}",
            serde_json::to_string_pretty(&response).expect("should encode to JSON")
        );
    }
}
