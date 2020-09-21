use std::io::Write;

use clap::{App, ArgGroup, ArgMatches, SubCommand};

use super::creation_common;
use crate::command::ClientCommand;

pub struct MakeDeploy;

impl<'a, 'b> ClientCommand<'a, 'b> for MakeDeploy {
    const NAME: &'static str = "make-deploy";
    const ABOUT: &'static str = "Constructs a deploy and outputs it to a file \
    or stdout.  As a file, the deploy can subsequently be signed by other \
    parties and sent to a node.";

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
            )
            .arg(creation_common::output::arg(display_order));
        creation_common::apply_common_creation_options(subcommand)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);

        let session = creation_common::parse_session_info(matches);
        let deploy = creation_common::parse_deploy(matches, session);

        let output_path = creation_common::output::get(matches);

        let mut out = creation_common::output::output_or_stdout(matches)
            .unwrap_or_else(|e| panic!("error opening output {:?}", e));
        let content = serde_json::to_string(&deploy).expect("unable to serialize deploy");

        match out.write_all(content.as_bytes()) {
            Ok(()) if output_path.is_some() => {
                println!("Successfully wrote deploy to {:?}", output_path);
            }
            Ok(()) => {}
            Err(err) => panic!("Error writing deploy to {:?}: {:?}", output_path, err)
        }
    }
}
