use clap::{App, ArgMatches, SubCommand};

use super::creation_common::{self, DisplayOrder};
use crate::{command::ClientCommand, common};

pub struct SendDeploy;

impl<'a, 'b> ClientCommand<'a, 'b> for SendDeploy {
    const NAME: &'static str = "send-deploy";
    const ABOUT: &'static str =
        "Reads a previously-saved deploy from a file and sends it to the network for execution";

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
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let mut verbosity_level = common::verbose::get(matches);
        let input_path = creation_common::input::get(matches);

        let response = casper_client::send_deploy_file(
            maybe_rpc_id,
            node_address,
            verbosity_level,
            &input_path,
        )
        .unwrap_or_else(|error| panic!("response error: {}", error));

        if verbosity_level == 0 {
            verbosity_level += 1
        }
        casper_client::pretty_print_at_level(&response, verbosity_level);
    }
}
