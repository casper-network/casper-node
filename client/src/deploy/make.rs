use clap::{App, ArgMatches, SubCommand};

use casper_client::DeployStrParams;

use super::creation_common;
use crate::{command::ClientCommand, common};

pub struct MakeDeploy;

impl<'a, 'b> ClientCommand<'a, 'b> for MakeDeploy {
    const NAME: &'static str = "make-deploy";
    const ABOUT: &'static str =
        "Creates a deploy and outputs it to a file or stdout. As a file, the deploy can \
        subsequently be signed by other parties using the 'sign-deploy' subcommand and then sent \
        to the network for execution using the 'send-deploy' subcommand";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .arg(creation_common::output::arg())
            .display_order(display_order);
        let subcommand = creation_common::apply_common_session_options(subcommand);
        let subcommand = creation_common::apply_common_payment_options(subcommand);
        creation_common::apply_common_creation_options(subcommand, false)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);

        let secret_key = common::secret_key::get(matches);
        let timestamp = creation_common::timestamp::get(matches);
        let ttl = creation_common::ttl::get(matches);
        let gas_price = creation_common::gas_price::get(matches);
        let dependencies = creation_common::dependencies::get(matches);
        let chain_name = creation_common::chain_name::get(matches);

        let session_str_params = creation_common::session_str_params(matches);
        let payment_str_params = creation_common::payment_str_params(matches);

        let maybe_output_path = creation_common::output::get(matches);

        casper_client::make_deploy(
            maybe_output_path.unwrap_or_default(),
            DeployStrParams {
                secret_key,
                timestamp,
                ttl,
                dependencies,
                gas_price,
                chain_name,
            },
            session_str_params,
            payment_str_params,
        )
        .unwrap_or_else(|err| panic!("unable to make deploy {:?}", err));
    }
}
