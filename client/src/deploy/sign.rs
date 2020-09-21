use std::{
    io::Write,
    fs
};

use clap::{App, ArgMatches, SubCommand};

use super::creation_common;
use crate::{command::ClientCommand, common};

use casper_node::types::Deploy;

pub struct SignDeploy;

impl<'a, 'b> ClientCommand<'a, 'b> for SignDeploy {
    const NAME: &'static str = "sign-deploy";
    const ABOUT: &'static str = "Cryptographically signs a deploy. The signature \
    is appended to existing approvals.";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::secret_key::arg(display_order))
            .arg(creation_common::input::arg(display_order))
            .arg(creation_common::output::arg(display_order));
        creation_common::apply_common_creation_options(subcommand)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);
        let input_path = creation_common::input::get(matches);
        let output_path = creation_common::output::get(matches);

        let input = fs::read(&input_path)
            .unwrap_or_else(|e| panic!("unable to open input file {} - {:?}", input_path, e));

        let input = String::from_utf8(input).unwrap();
        let mut deploy = serde_json::from_str::<Deploy>(&input)
            .unwrap_or_else(|e| panic!("unable to deserialize deploy file {} - {:?}", input_path, e));

        let secret_key = common::secret_key::get(matches);
        let mut rng = rand::thread_rng();
        deploy.sign(&secret_key, &mut rng);

        let mut out = creation_common::output::output_or_stdout(matches)
            .unwrap_or_else(|e| panic!("error opening output {:?}", e));
        let content = serde_json::to_string(&deploy).expect("unable to serialize deploy");

        match out.write_all(content.as_bytes()) {
            Ok(()) if output_path.is_some() => {
                println!("Successfully wrote deploy to {:?}", output_path);
            }
            Ok(()) => {}
            Err(err) => panic!("Error writing deploy to {:?}: {:?}", output_path, err),
        }
    }
}
