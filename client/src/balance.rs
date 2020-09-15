use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};
use futures::executor;

use crate::{command::ClientCommand, common};
use common::RpcClient;

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    BlockHash,
    Key,
    Path,
}

pub struct GetBalance {}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BalanceArgs {
    pub key: String,
    pub path: Vec<String>,
    #[serde(default)]
    pub block_hash: Option<String>,
}

mod balance_args {
    use super::*;

    mod names {
        pub const KEY: &str = "key";
        pub const PATH: &str = "path";
        pub const BLOCK_HASH: &str = "block_hash";
    }

    mod value_names {
        pub const KEY: &str = "HEX STRING";
        pub const PATH: &str = "key1,key2,key3...";
        pub const BLOCK_HASH: &str = "HEX STRING or null";
    }

    mod help {
        pub const KEY: &str = "key";
        pub const PATH: &str = "Comma-delimited block hashes";
        pub const BLOCK_HASH: &str = "block_hash, if omitted will use last";
    }

    pub(super) fn key() -> Arg<'static, 'static> {
        Arg::with_name(names::KEY)
            .required(true)
            .value_name(value_names::KEY)
            .help(help::KEY)
            .display_order(DisplayOrder::Key as usize)
    }

    pub(super) fn path() -> Arg<'static, 'static> {
        Arg::with_name(names::PATH)
            .required(true)
            .value_name(value_names::PATH)
            .help(help::PATH)
            .use_delimiter(true)
            .multiple(true)
            .display_order(DisplayOrder::Path as usize)
    }

    pub(super) fn block_hash() -> Arg<'static, 'static> {
        Arg::with_name(names::BLOCK_HASH)
            .required(false)
            .last(true) // because this is optional, it must come last as per clap's rules
            .value_name(value_names::BLOCK_HASH)
            .help(help::BLOCK_HASH)
            .display_order(DisplayOrder::BlockHash as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> BalanceArgs {
        let key = matches
            .value_of(names::KEY)
            .unwrap_or_else(|| panic!("should have {} arg", names::KEY))
            .to_string();

        let path = matches
            .values_of(names::PATH)
            .unwrap_or_else(|| panic!("should have {} arg", names::PATH))
            .map(str::to_string)
            .collect::<Vec<_>>();

        let block_hash = matches.value_of(names::BLOCK_HASH).map(str::to_string);

        BalanceArgs {
            block_hash,
            key,
            path,
        }
    }
}

impl RpcClient for GetBalance {
    const RPC_METHOD: &'static str = "state_get_item";
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetBalance {
    const NAME: &'static str = "get-balance";
    const ABOUT: &'static str = "Retrieves a stored balance";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(balance_args::key())
            .arg(balance_args::path())
            .arg(balance_args::block_hash())
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let args = balance_args::get(matches);

        let url = format!("{}/{}", node_address, common::RPC_API_PATH);
        let rpc_req = jsonrpc_lite::JsonRpc::request_with_params(
            42, // TODO: perhaps we should validate that this conforms
            Self::RPC_METHOD,
            serde_json::json!(args),
        );
        let client = reqwest::Client::new();

        let body = executor::block_on(async {
            client
                .post(&url)
                .json(&rpc_req)
                .send()
                .await
                .unwrap_or_else(|error| panic!("should get response from node: {}", error))
                .bytes()
                .await
                .unwrap_or_else(|error| panic!("should get bytes from node response: {}", error))
        });

        let json_encoded = str::from_utf8(body.as_ref())
            .unwrap_or_else(|error| panic!("should parse node response as JSON: {}", error));

        let rpc_res = match jsonrpc_lite::JsonRpc::parse(json_encoded) {
            Ok(res) => res,
            Err(error) => {
                panic!("node response error: {}", error);
            }
        };

        println!("{:?}", rpc_res);
    }
}
