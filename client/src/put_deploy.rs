use std::{process, str::FromStr};

use clap::{App, Arg, ArgMatches, SubCommand};
use futures::executor;
use rand::Rng;
use reqwest::{Client, StatusCode};

use casperlabs_node::{
    components::contract_runtime::core::engine_state::executable_deploy_item::ExecutableDeployItem,
    crypto::{
        asymmetric_key::{self, PublicKey},
        hash::Digest,
    },
    types::{Deploy, DeployHeader, Timestamp},
};
use casperlabs_types::{bytesrepr::ToBytes, RuntimeArgs};

use crate::common;

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    SecretKey,
    Timestamp,
    Ttl,
    GasPrice,
    ChainName,
    SessionCode,
    SessionArgs,
    PaymentCode,
    PaymentArgs,
}

/// Handles providing the arg for and retrieval of the timestamp.
mod timestamp {
    use super::*;

    const ARG_NAME: &str = "timestamp";
    const ARG_VALUE_NAME: &str = "MILLISECONDS";
    const ARG_HELP: &str =
        "Timestamp as the number of milliseconds since the Unix epoch. If not provided, the \
        current time will be used";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Timestamp as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Timestamp {
        matches
            .value_of(ARG_NAME)
            .map_or_else(Timestamp::now, |value| {
                Timestamp::from_str(value)
                    .unwrap_or_else(|error| panic!("should parse {}: {}", ARG_NAME, error))
            })
    }
}

/// Handles providing the arg for and retrieval of the time to live.
mod ttl {
    use super::*;

    const ARG_NAME: &str = "ttl";
    const ARG_VALUE_NAME: &str = "MILLISECONDS";
    const ARG_DEFAULT: &str = "3600000";
    const ARG_HELP: &str =
        "Time (in milliseconds) that the deploy will remain valid for. A deploy can only be \
        included in a block between `timestamp` and `timestamp + ttl`.";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .default_value(ARG_DEFAULT)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Ttl as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> u32 {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .parse()
            .unwrap_or_else(|error| panic!("should parse {}: {}", ARG_NAME, error))
    }
}

/// Handles providing the arg for and retrieval of the gas price.
mod gas_price {
    use super::*;

    const ARG_NAME: &str = "gas-price";
    const ARG_VALUE_NAME: &str = "INTEGER";
    const ARG_DEFAULT: &str = "10";
    const ARG_HELP: &str =
        "Conversion rate between the cost of Wasm opcodes and the motes sent by the payment code";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .default_value(ARG_DEFAULT)
            .help(ARG_HELP)
            .display_order(DisplayOrder::GasPrice as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> u64 {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .parse()
            .unwrap_or_else(|error| panic!("should parse {}: {}", ARG_NAME, error))
    }
}

/// Handles providing the arg for and retrieval of the chain name.
mod chain_name {
    use super::*;

    const ARG_NAME: &str = "chain-name";
    const ARG_VALUE_NAME: &str = "NAME";
    const ARG_DEFAULT: &str = "Test";
    const ARG_HELP: &str =
        "Name of the chain, to avoid the deploy from being accidentally or maliciously included in \
        a different chain";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .default_value(ARG_DEFAULT)
            .help(ARG_HELP)
            .display_order(DisplayOrder::ChainName as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

/// Handles providing the arg for and retrieval of the session code bytes.
mod session {
    use super::*;

    const ARG_NAME: &str = "session-path";
    const ARG_SHORT: &str = "s";
    const ARG_VALUE_NAME: &str = "PATH";
    const ARG_HELP: &str = "Path to the compiled Wasm session code";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .short(ARG_SHORT)
            .long(ARG_NAME)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::SessionCode as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Vec<u8> {
        common::read_file(
            matches
                .value_of(ARG_NAME)
                .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME)),
        )
    }
}

/// Handles providing the arg for and retrieval of the session args.
mod session_args {
    use super::*;

    const ARG_NAME: &str = "session-args";
    const ARG_VALUE_NAME: &str = "JSON ARRAY";
    const ARG_DEFAULT: &str = "[]";
    const ARG_HELP: &str = concat!(
        r#"JSON encoded array of session args, e.g.: '[{"name": "amount", "value": {"long_value": "#,
        r#"123456}}]'"#
    );

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .default_value(ARG_DEFAULT)
            .help(ARG_HELP)
            .display_order(DisplayOrder::SessionArgs as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> RuntimeArgs {
        let args = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));
        // TODO - fix
        RuntimeArgs::new()
    }
}

/// Handles providing the arg for and retrieval of the payment code bytes.
mod payment {
    use super::*;

    const ARG_NAME: &str = "payment-path";
    const ARG_SHORT: &str = "p";
    const ARG_VALUE_NAME: &str = "PATH";
    const ARG_HELP: &str = "Path to the compiled Wasm payment code";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .short(ARG_SHORT)
            .long(ARG_NAME)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::PaymentCode as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Vec<u8> {
        common::read_file(
            matches
                .value_of(ARG_NAME)
                .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME)),
        )
    }
}

/// Handles providing the arg for and retrieval of the payment args.
mod payment_args {
    use super::*;

    const ARG_NAME: &str = "payment-args";
    const ARG_VALUE_NAME: &str = "JSON ARRAY";
    const ARG_HELP: &str = concat!(
        r#"JSON encoded array of payment args, e.g.: '[{"name": "amount", "value": {"big_int": "#,
        r#"{"value": "123456", "bit_width": 512}}}]'"#
    );

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::PaymentArgs as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> RuntimeArgs {
        let args = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));
        // TODO - fix
        RuntimeArgs::new()
    }
}

pub struct PutDeploy {}

impl<'a, 'b> crate::Subcommand<'a, 'b> for PutDeploy {
    const NAME: &'static str = "put-deploy";
    const ABOUT: &'static str = "Stores a new random deploy";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::secret_key::arg(DisplayOrder::SecretKey as usize))
            .arg(timestamp::arg())
            .arg(ttl::arg())
            .arg(gas_price::arg())
            .arg(chain_name::arg())
            .arg(session::arg())
            .arg(session_args::arg())
            .arg(payment::arg())
            .arg(payment_args::arg())
        // TODO: There are also deploy dependencies but this whole structure is subject to changes.
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let secret_key = common::secret_key::get(matches);
        let timestamp = timestamp::get(matches);
        let ttl = ttl::get(matches);
        let gas_price = gas_price::get(matches);
        let chain_name = chain_name::get(matches);
        let session_bytes = session::get(matches);
        let session_args = session_args::get(matches);
        let payment_bytes = payment::get(matches);
        let payment_args = payment_args::get(matches);

        let public_key = PublicKey::from(&secret_key);
        let header = DeployHeader {
            account: public_key,
            timestamp: timestamp.millis(),
            gas_price,
            body_hash: [1; 32].into(),
            ttl_millis: ttl,
            dependencies: vec![],
            chain_name,
        };

        let deploy_hash_bytes: [u8; 32] = rand::thread_rng().gen();
        let deploy_hash = Digest::from(deploy_hash_bytes);

        let session = ExecutableDeployItem::ModuleBytes {
            module_bytes: session_bytes,
            args: session_args.to_bytes().expect("should serialize"),
        };

        let payment = ExecutableDeployItem::ModuleBytes {
            module_bytes: payment_bytes,
            args: payment_args.to_bytes().expect("should serialize"),
        };

        let msg = b"Message"; // TODO
        let sig = asymmetric_key::sign(msg, &secret_key, &public_key);

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
            println!("Stored deploy with deploy-hash:\n{:?}", deploy_hash);
        } else {
            eprintln!("Storing {} failed\n{:?}", deploy_hash, response);
            process::exit(1);
        }
    }
}
