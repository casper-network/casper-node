use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};

use crate::{command::ClientCommand, common, params::StateGetItem, RpcClient};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    GlobalStateHash,
    Key,
    Path,
}

/// Handles providing the arg for and retrieval of the global state hash.
mod global_state_hash {
    use super::*;

    const ARG_NAME: &str = "global-state-hash";
    const ARG_SHORT: &str = "g";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str = "Hex-encoded global state hash";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::GlobalStateHash as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

/// Handles providing the arg for and retrieval of the key.
mod key {
    use super::*;

    const ARG_NAME: &str = "key";
    const ARG_SHORT: &str = "k";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING";
    const ARG_HELP: &str =
        "The base key for the query.  This must be a properly formatted account hash, contract \
        address hash or URef.  The format for each respectively is \
        \"account-account_hash-<HEX STRING>\", \"hash-<HEX STRING>\" and \
        \"uref-<HEX STRING>-<THREE DIGIT INTEGER>\"";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Key as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

/// Handles providing the arg for and retrieval of the key.
mod path {
    use super::*;

    const ARG_NAME: &str = "path";
    const ARG_SHORT: &str = "p";
    const ARG_VALUE_NAME: &str = "PATH/FROM/BASE/KEY";
    const ARG_HELP: &str = "The path from the base key for the query";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Path as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Vec<String> {
        match matches.value_of(ARG_NAME) {
            Some("") | None => return vec![],
            Some(path) => path.split('/').map(ToString::to_string).collect(),
        }
    }
}

pub struct QueryState {}

impl RpcClient for QueryState {
    const RPC_METHOD: &'static str = "state_get_item";
}

impl<'a, 'b> ClientCommand<'a, 'b> for QueryState {
    const NAME: &'static str = "query-state";
    const ABOUT: &'static str = "Retrieves a stored value from global state";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(global_state_hash::arg())
            .arg(key::arg())
            .arg(path::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let global_state_hash = global_state_hash::get(matches);
        let key = key::get(matches);
        let path = path::get(matches);

        let params = StateGetItem::new(global_state_hash, key, path);

        let response_value = Self::request_sync(&node_address, params)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);
    }
}
