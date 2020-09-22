use clap::{App, ArgMatches, SubCommand};

use super::creation_common;
use crate::{command::ClientCommand, common};

enum DisplayOrder {
    SecretKey,
    Input,
    Output,
}

pub struct SignDeploy;

impl<'a, 'b> ClientCommand<'a, 'b> for SignDeploy {
    const NAME: &'static str = "sign-deploy";
    const ABOUT: &'static str = "Cryptographically signs a deploy and appends signature to existing approvals";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::secret_key::arg(DisplayOrder::SecretKey as usize))
            .arg(creation_common::input::arg(DisplayOrder::Input as usize))
            .arg(creation_common::output::arg(DisplayOrder::Output as usize))
    }

    fn run(matches: &ArgMatches<'_>) {
        let input_path = creation_common::input::get(matches);
        let mut deploy = creation_common::input::read_deploy(&input_path);
        let secret_key = common::secret_key::get(matches);
        let mut rng = rand::thread_rng();
        deploy.sign(&secret_key, &mut rng);
        creation_common::output::write_deploy(&deploy, creation_common::output::get(matches));
    }
}