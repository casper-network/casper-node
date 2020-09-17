use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};

use crate::{command::ClientCommand, common, rpc::RpcClient};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    GlobalStateHash,
    Key,
    Path,
}

pub struct GetBalance {}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BalanceArgs {
    pub global_state_hash: String,
    pub key: String,
    pub path: Vec<String>,
}

mod balance_args {
    use super::*;

    mod names {
        pub const KEY: &str = "key";
        pub const PATH: &str = "path";
        pub const GLOBAL_STATE_HASH: &str = "global_state_hash";
    }

    mod value_names {
        pub const KEY: &str = "HEX STRING";
        pub const PATH: &str = "key1,key2,key3...";
        pub const GLOBAL_STATE_HASH: &str = "HEX STRING";
    }

    mod help {
        pub const KEY: &str = "key";
        pub const PATH: &str = "Comma-delimited block hashes";
        pub const GLOBAL_STATE_HASH: &str = "global state hash";
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

    pub(super) fn global_state_hash() -> Arg<'static, 'static> {
        Arg::with_name(names::GLOBAL_STATE_HASH)
            .required(true)
            .value_name(value_names::GLOBAL_STATE_HASH)
            .help(help::GLOBAL_STATE_HASH)
            .display_order(DisplayOrder::GlobalStateHash as usize)
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

        let global_state_hash = matches
            .value_of(names::GLOBAL_STATE_HASH)
            .map(str::to_string)
            .unwrap_or_else(|| panic!("should have {} arg", names::GLOBAL_STATE_HASH));

        BalanceArgs {
            global_state_hash,
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
            .arg(balance_args::global_state_hash())
            .arg(balance_args::key())
            .arg(balance_args::path())
    }

    fn run(matches: &ArgMatches<'_>) {
        let _node_address = common::node_address::get(matches);
        let _args = balance_args::get(matches);
        todo!();
        // let res = match Self::request_sync(&node_address, &args) {
        //     Ok(res) => res,
        //     Err(err) => {
        //         println!("error {:?}", err);
        //         return;
        //     }
        // };
        // println!("{}", res);
    }
}
