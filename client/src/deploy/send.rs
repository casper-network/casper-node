use clap::{App, ArgMatches, SubCommand};

use super::creation_common;
use crate::{command::ClientCommand, common};

pub struct SendDeploy;

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
        let response_value = client_lib::deploy::send_deploy_file(&node_address, &input_path)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{:?}", response_value);
    }
}
