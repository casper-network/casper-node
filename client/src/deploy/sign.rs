use clap::{App, ArgMatches, SubCommand};

use casper_client::deploy::DeployExt;
use casper_node::types::Deploy;

use super::creation_common;
use crate::{command::ClientCommand, common};

pub struct SignDeploy;

impl<'a, 'b> ClientCommand<'a, 'b> for SignDeploy {
    const NAME: &'static str = "sign-deploy";
    const ABOUT: &'static str =
        "Cryptographically signs a deploy and appends signature to existing approvals";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::secret_key::arg(
                creation_common::DisplayOrder::SecretKey as usize,
            ))
            .arg(creation_common::input::arg())
            .arg(creation_common::output::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let input_path = creation_common::input::get(matches);
        let secret_key = common::secret_key::get(matches);
        let maybe_output = creation_common::output::get(matches);
        Deploy::sign_deploy_file(&input_path, secret_key, maybe_output).unwrap_or_else(
            move |err| panic!("error writing deploy to {:?}: {}", maybe_output, err),
        );
    }
}
