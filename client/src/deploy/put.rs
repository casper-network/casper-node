use clap::{App, ArgGroup, ArgMatches, SubCommand};

use super::creation_common;
use crate::{command::ClientCommand, common, RpcClient};

pub struct PutDeploy {}

impl RpcClient for PutDeploy {
    const RPC_METHOD: &'static str = "account_put_deploy";
}

impl<'a, 'b> ClientCommand<'a, 'b> for PutDeploy {
    const NAME: &'static str = "put-deploy";
    const ABOUT: &'static str = "Creates a new deploy and sends it to the network for execution";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(creation_common::session::arg())
            .arg(creation_common::arg_simple::session::arg())
            .arg(creation_common::args_complex::session::arg())
            // Group the session-arg args so only one style is used to ensure consistent ordering.
            .group(
                ArgGroup::with_name("session-args")
                    .arg(creation_common::arg_simple::session::ARG_NAME)
                    .arg(creation_common::args_complex::session::ARG_NAME)
                    .required(false),
            );
        creation_common::apply_common_creation_options(subcommand)
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let session = creation_common::parse_session_info(matches);
        let params = creation_common::construct_deploy(matches, session);

        let response_value = Self::request_with_map_params(&node_address, params)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
