use std::process;

use clap::{App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};
use futures::executor;
use rand::Rng;
use reqwest::{Client, StatusCode};

use casperlabs_node::{
    components::contract_runtime::core::engine_state::executable_deploy_item::ExecutableDeployItem,
    crypto::{
        asymmetric_key::{self, PublicKey},
        hash::Digest,
    },
    types::{Deploy, DeployHeader},
};
use casperlabs_types::{bytesrepr::ToBytes, RuntimeArgs, URef, U512};

use super::creation_common::*;
use crate::common;

/// Handles providing the arg for and retrieval of the transfer amount.
pub(super) mod amount {
    use super::*;

    const ARG_NAME: &str = "amount";
    const ARG_SHORT: &str = "a";
    const ARG_VALUE_NAME: &str = "512-BIT INTEGER";
    const ARG_HELP: &str = "The number of motes to transfer";

    pub(in crate::deploy) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferAmount as usize)
    }

    pub(in crate::deploy) fn get(matches: &ArgMatches) -> U512 {
        let value = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));
        U512::from_dec_str(value).unwrap_or_else(|error| {
            panic!("can't parse --{} {} as U512: {}", ARG_NAME, value, error)
        })
    }
}

/// Handles providing the arg for and retrieval of the source purse.
mod source_purse {
    use super::*;

    pub(super) const ARG_NAME: &str = "source-purse";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str =
        "Hex-encoded URef of the source purse. If this is omitted, the main purse of the account \
        creating this transfer will be used as the source purse";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferSourcePurse as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Option<URef> {
        matches.value_of(ARG_NAME).map(|value| {
            URef::from_formatted_str(value).unwrap_or_else(|error| {
                panic!("can't parse --{} {} as URef: {:?}", ARG_NAME, value, error)
            })
        })
    }
}

/// Handles providing the arg for and retrieval of the target account.
mod target_account {
    use super::*;

    pub(super) const ARG_NAME: &str = "target-account";
    const ARG_SHORT: &str = "t";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str =
        "Hex-encoded public key of the account from which the main purse will be used as the \
        target.";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .conflicts_with(super::target_purse::ARG_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferTargetAccount as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Option<PublicKey> {
        matches.value_of(ARG_NAME).map(|value| {
            PublicKey::from_hex(value).unwrap_or_else(|error| {
                panic!(
                    "can't parse --{} {} as PublicKey: {:?}",
                    ARG_NAME, value, error
                )
            })
        })
    }
}

/// Handles providing the arg for and retrieval of the target purse.
mod target_purse {
    use super::*;

    pub(super) const ARG_NAME: &str = "target-purse";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str = "Hex-encoded URef of the target purse";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::TransferTargetPurse as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Option<URef> {
        matches.value_of(ARG_NAME).map(|value| {
            URef::from_formatted_str(value).unwrap_or_else(|error| {
                panic!("can't parse --{} {} as URef: {:?}", ARG_NAME, value, error)
            })
        })
    }
}

fn create_transfer_args(matches: &ArgMatches) -> RuntimeArgs {
    const TRANSFER_ARG_AMOUNT: &str = "amount";
    const TRANSFER_ARG_SOURCE: &str = "source";
    const TRANSFER_ARG_TARGET: &str = "target";

    let mut runtime_args = RuntimeArgs::new();
    runtime_args.insert(TRANSFER_ARG_AMOUNT, amount::get(matches));

    if let Some(source_purse) = source_purse::get(matches) {
        runtime_args.insert(TRANSFER_ARG_SOURCE, source_purse);
    }

    match (target_account::get(matches), target_purse::get(matches)) {
        (Some(target_account), None) => {
            let target_account_hash = target_account.to_account_hash().value();
            runtime_args.insert(TRANSFER_ARG_TARGET, target_account_hash);
        }
        (None, Some(target_purse)) => {
            runtime_args.insert(TRANSFER_ARG_TARGET, target_purse);
        }
        _ => unreachable!("should have a target"),
    }

    runtime_args
}

pub struct Transfer {}

impl<'a, 'b> crate::Subcommand<'a, 'b> for Transfer {
    const NAME: &'static str = "transfer";
    const ABOUT: &'static str = "Transfers funds between purses";

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
            .arg(amount::arg())
            .arg(source_purse::arg())
            .arg(target_account::arg())
            .arg(target_purse::arg())
            .arg(timestamp::arg())
            .arg(ttl::arg())
            .arg(gas_price::arg())
            .arg(chain_name::arg())
            .arg(standard_payment::arg())
            .arg(payment::arg())
            .arg(arg_simple::payment::arg())
            .arg(args_complex::payment::arg())
            // Group the target args to ensure one is given.
            .group(
                ArgGroup::with_name("target-args")
                    .arg(target_account::ARG_NAME)
                    .arg(target_purse::ARG_NAME)
                    .required(false),
            )
            // Group the payment-arg args so only one style is used to ensure consistent ordering.
            .group(
                ArgGroup::with_name("payment-args")
                    .arg(arg_simple::payment::ARG_NAME)
                    .arg(args_complex::payment::ARG_NAME)
                    .required(false),
            )
            // Group payment-amount, payment-path and show-arg-examples so that we can require only
            // one of these.
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

        let payment = parse_payment_info(matches);
        let transfer_args = create_transfer_args(matches)
            .to_bytes()
            .expect("should serialize");
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };

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
