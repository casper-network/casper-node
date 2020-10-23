use casper_client::{DeployStrParams, PaymentStrParams, SessionStrParams};
use clap::{App, ArgMatches, SubCommand};

use casper_node::rpcs::account::PutDeploy;

use super::creation_common::{self, DisplayOrder};
use crate::{command::ClientCommand, common};

impl<'a, 'b> ClientCommand<'a, 'b> for PutDeploy {
    const NAME: &'static str = "put-deploy";
    const ABOUT: &'static str = "Creates a new deploy and sends it to the network for execution";

    fn build(display_order: usize) -> App<'a, 'b> {
        let subcommand = SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize));
        let subcommand = creation_common::apply_common_session_options(subcommand);
        let subcommand = creation_common::apply_common_payment_options(subcommand);
        creation_common::apply_common_creation_options(subcommand, true)
    }

    fn run(matches: &ArgMatches<'_>) {
        creation_common::show_arg_examples_and_exit_if_required(matches);

        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let verbose = common::verbose::get(matches);

        let secret_key = common::secret_key::get(matches);
        let timestamp = creation_common::timestamp::get(matches);
        let ttl = creation_common::ttl::get(matches);
        let gas_price = creation_common::gas_price::get(matches);
        let dependencies = creation_common::dependencies::get(matches);
        let chain_name = creation_common::chain_name::get(matches);

        let session_args_simple = creation_common::arg_simple::session::get(matches);
        let session_args_complex = creation_common::args_complex::session::get(matches);
        let session_name = creation_common::session_package_name::get(matches);
        let session_hash = creation_common::session_hash::get(matches);
        let session_version = creation_common::session_version::get(matches);
        let session_package_name = creation_common::session_package_name::get(matches);
        let session_package_hash = creation_common::session_package_hash::get(matches);
        let session_path = creation_common::session_path::get(matches);
        let session_entry_point = creation_common::session_entry_point::get(matches);

        let payment_amount = creation_common::standard_payment_amount::get(matches);
        let payment_args_simple = creation_common::arg_simple::payment::get(matches);
        let payment_args_complex = creation_common::args_complex::payment::get(matches);
        let payment_name = creation_common::payment_name::get(matches);
        let payment_hash = creation_common::payment_hash::get(matches);
        let payment_version = creation_common::payment_version::get(matches);
        let payment_package_name = creation_common::payment_package_name::get(matches);
        let payment_package_hash = creation_common::payment_package_hash::get(matches);
        let payment_path = creation_common::payment_path::get(matches);
        let payment_entry_point = creation_common::payment_entry_point::get(matches);

        casper_client::put_deploy(
            maybe_rpc_id,
            node_address,
            verbose,
            DeployStrParams {
                secret_key,
                timestamp,
                ttl,
                dependencies: &dependencies,
                gas_price,
                chain_name,
            },
            SessionStrParams {
                session_hash,
                session_name,
                session_package_hash,
                session_package_name,
                session_path,
                session_args_simple: &session_args_simple,
                session_args_complex,
                session_version,
                session_entry_point,
            },
            PaymentStrParams {
                payment_amount,
                payment_hash,
                payment_name,
                payment_package_hash,
                payment_package_name,
                payment_path,
                payment_args_simple: &payment_args_simple,
                payment_args_complex,
                payment_version,
                payment_entry_point,
            },
        )
        .unwrap_or_else(|err| panic!("unable to put deploy {:?}", err));
    }
}
