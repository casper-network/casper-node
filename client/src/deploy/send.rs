use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::{
    account::{PutDeploy, PutDeployParams},
    RpcWithParams,
};

use super::creation_common::{self, DisplayOrder};
use crate::{command::ClientCommand, common, RpcClient};

pub struct SendDeploy;

impl RpcClient for SendDeploy {
    const RPC_METHOD: &'static str = PutDeploy::METHOD;
}

impl<'a, 'b> ClientCommand<'a, 'b> for SendDeploy {
    const NAME: &'static str = "send-deploy";
    const ABOUT: &'static str = "Sends a deploy to the network for execution";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(creation_common::input::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let verbose = common::verbose::get(matches);
        let node_address = common::node_address::get(matches);
        let rpc_id = common::rpc_id::get(matches);
        let input_path = creation_common::input::get(matches);
        let deploy = creation_common::input::read_deploy(&input_path);
        let params = PutDeployParams { deploy };
        let response = Self::request_with_map_params(verbose, &node_address, rpc_id, params);
        println!(
            "{}",
            serde_json::to_string_pretty(&response).expect("should encode to JSON")
        );
    }
}
