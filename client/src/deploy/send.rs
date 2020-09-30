use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::account::PutDeployParams;

use super::creation_common;
use crate::{command::ClientCommand, common, RpcClient};

pub struct SendDeploy;

impl RpcClient for SendDeploy {
    const RPC_METHOD: &'static str = "account_put_deploy";
}

impl<'a, 'b> ClientCommand<'a, 'b> for SendDeploy {
    const NAME: &'static str = "send-deploy";
    const ABOUT: &'static str = "Sends a deploy to the network for execution";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                creation_common::DisplayOrder::NodeAddress as usize,
            ))
            .arg(creation_common::input::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let input_path = creation_common::input::get(matches);
        let deploy = creation_common::input::read_deploy(&input_path);
        let params = PutDeployParams { deploy };
        let response_value = Self::request_with_map_params(&node_address, params)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
