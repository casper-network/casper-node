use clap::{App, ArgGroup, ArgMatches, SubCommand};

use super::creation_common;

pub struct PutDeploy {}

impl<'a, 'b> crate::Subcommand<'a, 'b> for PutDeploy {
    const NAME: &'static str = "put-deploy";
    const ABOUT: &'static str = "Stores a new random deploy";

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
        let session = creation_common::parse_session_info(matches);
        creation_common::construct_and_send_deploy_to_node(matches, session)
    }
}
