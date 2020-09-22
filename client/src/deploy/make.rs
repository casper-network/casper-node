use clap::{App, ArgMatches, SubCommand};

use super::creation_common;
use crate::command::ClientCommand;

pub struct MakeDeploy;

impl<'a, 'b> ClientCommand<'a, 'b> for MakeDeploy {
    const NAME: &'static str = "make-deploy";
    const ABOUT: &'static str = "Constructs a deploy and outputs it to a file \
    or stdout. As a file, the deploy can subsequently be signed by other \
    parties and sent to a node, or signed with the sign-deploy subcommand";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order);
        let subcommand = creation_common::apply_common_session_options(subcommand)
            .arg(creation_common::output::arg(display_order));
        creation_common::apply_common_creation_options(subcommand, false)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);
        let session = creation_common::parse_session_info(matches);
        let deploy = creation_common::parse_deploy(matches, session);
        creation_common::output::write_deploy(&deploy, creation_common::output::get(matches));
    }
}
