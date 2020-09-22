use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::{account::PutDeploy, RpcWithParams};

use super::creation_common;
use crate::{command::ClientCommand, common, RpcClient};

impl RpcClient for PutDeploy {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl<'a, 'b> ClientCommand<'a, 'b> for PutDeploy {
    const NAME: &'static str = "put-deploy";
    const ABOUT: &'static str = "Creates a new deploy and sends it to the network for execution";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order);
        let subcommand = creation_common::apply_common_session_options(subcommand);
        let subcommand = subcommand
            .arg(creation_common::deploy_args::DEPENDENCIES_ARG.arg())
            .arg(creation_common::deploy_args::SESSION_PACKAGE_HASH_ARG.arg())
            .arg(creation_common::deploy_args::SESSION_PACKAGE_NAME_ARG.arg())
            .arg(creation_common::deploy_args::SESSION_HASH_ARG.arg())
            .arg(creation_common::deploy_args::SESSION_NAME_ARG.arg())
            .arg(creation_common::deploy_args::SESSION_ENTRY_POINT_ARG.arg())
            .arg(creation_common::deploy_args::SESSION_VERSION_ARG.arg());

        creation_common::apply_common_creation_options(subcommand, true)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);

        let node_address = common::node_address::get(matches);
        let session = creation_common::parse_session_info(matches);
        let params = creation_common::construct_deploy(matches, session);

        let response_value = Self::request_with_map_params(&node_address, params)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
