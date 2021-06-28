use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_client::Error;
use casper_node::rpcs::state::QueryGlobalState;

use crate::{command::ClientCommand, common, Success};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    StateIdentifier,
    Hash,
    Key,
    Path,
}

mod state_identifier {
    use super::*;

    const ARG_NAME: &str = "identifier";
    const ARG_VALUE_NAME: &str = "STRING";
    const ARG_HELP: &str =
        "Identifies the hash value provided as either a block hash or state root hash. You must provide either a block hash or a state root hash";
    const BLOCK: &str = "block";
    const STATE_ROOT: &str = "state";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .required(true)
            .possible_value(BLOCK)
            .possible_value(STATE_ROOT)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::StateIdentifier as usize)
    }

    pub(super) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
    }
}

/// Handles providing the arg for and retrieval of either the block hash or state root hash.
mod hash {
    use super::*;

    const ARG_NAME: &str = "hash";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str = "Hex-encoded block hash or state root hash";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Hash as usize)
    }

    pub(super) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
    }
}

impl<'a, 'b> ClientCommand<'a, 'b> for QueryGlobalState {
    const NAME: &'static str = "global-state-query";
    const ABOUT: &'static str =
        "Retrieves a stored value from the network using either the state root hash or block hash";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(state_identifier::arg())
            .arg(hash::arg())
            .arg(common::key::arg(DisplayOrder::Key as usize))
            .arg(common::path::arg(DisplayOrder::Path as usize))
    }

    fn run(matches: &ArgMatches<'_>) -> Result<Success, Error> {
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let verbosity_level = common::verbose::get(matches);
        let state_identifier = state_identifier::get(matches);
        let hash = hash::get(matches);
        let key = common::key::get(matches)?;
        let path = common::path::get(matches);

        casper_client::global_state_query(
            maybe_rpc_id,
            node_address,
            verbosity_level,
            state_identifier,
            hash,
            &key,
            path,
        )
        .map(Success::from)
    }
}
