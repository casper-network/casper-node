use clap::{App, ArgMatches, SubCommand};

use casper_node::types::Deploy;

use super::creation_common;
use crate::{command::ClientCommand, common, RpcClient};

pub struct SendDeploy;

// TODO: rename casper_node account type PutDeploy => SendDeploy, and account_send_deploy
impl RpcClient for SendDeploy {
    const RPC_METHOD: &'static str = "account_put_deploy";
}

impl<'a, 'b> ClientCommand<'a, 'b> for SendDeploy {
    const NAME: &'static str = "send-deploy";
    const ABOUT: &'static str = "Deploy a smart contract file to the Casper network via an existing running node.";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(creation_common::input::arg(display_order));
        creation_common::apply_common_creation_options(subcommand)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);

        let node_address = common::node_address::get(matches);

        let input_path = creation_common::input::get(matches);
        let input = std::fs::read(&input_path)
            .unwrap_or_else(|e| panic!("unable to open file {} - {:?}", input_path, e));
        let input = String::from_utf8(input).unwrap();

        let deploy = serde_json::from_str::<Deploy>(&input)
            .unwrap_or_else(|e| panic!("unable to deserialize deploy file {} - {:?}", input, e));

        let params = creation_common::deploy_into_params(deploy);
        let response_value = Self::request_with_map_params(&node_address, params)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
