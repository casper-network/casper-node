use std::process;

use clap::{App, AppSettings, ArgGroup, ArgMatches, SubCommand};
use futures::executor;
use rand::Rng;
use reqwest::{Client, StatusCode};

use casperlabs_node::{
    crypto::{
        asymmetric_key::{self, PublicKey},
        hash::Digest,
    },
    types::{Deploy, DeployHeader},
};

use super::creation_common::*;
use crate::common;

pub struct PutDeploy {}

impl<'a, 'b> crate::Subcommand<'a, 'b> for PutDeploy {
    const NAME: &'static str = "put-deploy";
    const ABOUT: &'static str = "Stores a new random deploy";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .setting(AppSettings::NextLineHelp)
            .display_order(display_order)
            .arg(show_arg_examples::arg())
            .arg(
                common::node_address::arg(DisplayOrder::NodeAddress as usize)
                    .required_unless(show_arg_examples::ARG_NAME),
            )
            .arg(
                common::secret_key::arg(DisplayOrder::SecretKey as usize)
                    .required_unless(show_arg_examples::ARG_NAME),
            )
            .arg(timestamp::arg())
            .arg(ttl::arg())
            .arg(gas_price::arg())
            .arg(chain_name::arg())
            .arg(session::arg())
            .arg(arg_simple::session::arg())
            .arg(args_complex::session::arg())
            .arg(standard_payment::arg())
            .arg(payment::arg())
            .arg(arg_simple::payment::arg())
            .arg(args_complex::payment::arg())
            // Group the session-arg args so only one style is used to ensure consistent ordering.
            .group(
                ArgGroup::with_name("session-args")
                    .arg(arg_simple::session::ARG_NAME)
                    .arg(args_complex::session::ARG_NAME)
                    .required(false),
            )
            // Group the payment-arg args so only one style is used to ensure consistent ordering.
            .group(
                ArgGroup::with_name("payment-args")
                    .arg(arg_simple::payment::ARG_NAME)
                    .arg(args_complex::payment::ARG_NAME)
                    .required(false),
            )
            // Group payment-amount, payment-path and show-arg-examples so that we can require only one of these.
            .group(
                ArgGroup::with_name("required-payment-options")
                    .arg(standard_payment::ARG_NAME)
                    .arg(payment::ARG_NAME)
                    .arg(show_arg_examples::ARG_NAME)
                    .required(true),
            )
        // TODO: There are also deploy dependencies but this whole structure is subject to changes.
    }

    fn run(matches: &ArgMatches<'_>) {
        // If we printed the arg examples, exit the process.
        if show_arg_examples::get(matches) {
            process::exit(0);
        }

        let node_address = common::node_address::get(matches);
        let secret_key = common::secret_key::get(matches);
        let timestamp = timestamp::get(matches);
        let ttl = ttl::get(matches);
        let gas_price = gas_price::get(matches);
        let chain_name = chain_name::get(matches);

        let public_key = PublicKey::from(&secret_key);
        let header = DeployHeader {
            account: public_key,
            timestamp,
            gas_price,
            body_hash: [1; 32].into(),
            ttl,
            dependencies: vec![],
            chain_name,
        };

        let mut rng = rand::thread_rng();
        let deploy_hash_bytes: [u8; 32] = rng.gen();
        let deploy_hash = Digest::from(deploy_hash_bytes);

        let session = parse_session_info(matches);
        let payment = parse_payment_info(matches);

        let msg = b"Message"; // TODO
        let sig = asymmetric_key::sign(msg, &secret_key, &public_key, &mut rng);

        let deploy = Deploy::new(deploy_hash.into(), header, payment, session, vec![sig]);

        let body = deploy.to_json().expect("should serialize deploy to JSON");

        let client = Client::new();
        let url = format!("{}/{}", node_address, common::DEPLOY_API_PATH);

        let response = executor::block_on(async {
            client
                .post(&url)
                .body(body)
                .send()
                .await
                .unwrap_or_else(|error| panic!("should get response from node: {}", error))
        });

        if response.status() == StatusCode::OK {
            println!("Node received deploy with deploy-hash:\n{:?}", deploy_hash);
        } else {
            eprintln!("Storing {} failed\n{:?}", deploy_hash, response);
            process::exit(1);
        }
    }
}
