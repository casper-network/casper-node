use std::{fs, str};

use async_trait::async_trait;
use clap::{App, Arg, ArgGroup, ArgMatches, SubCommand};

use casper_client::{Error, GlobalStateStrParams};
use casper_node::rpcs::state::QueryGlobalState;

use crate::{command::ClientCommand, common, Success};

const ARG_HEX_STRING: &str = "HEX STRING";

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    BlockHash,
    StateRootHash,
    Key,
    Path,
}

mod state_root_hash {
    use super::*;

    pub(super) const ARG_NAME: &str = "state-root-hash";
    const ARG_SHORT: &str = "s";
    const ARG_VALUE_NAME: &str = ARG_HEX_STRING;
    const ARG_HELP: &str = "Hex-encoded hash of the state root";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::StateRootHash as usize)
    }

    pub fn get<'a>(matches: &'a ArgMatches) -> Option<&'a str> {
        matches.value_of(ARG_NAME)
    }
}

mod block_hash {
    use super::*;

    pub(super) const ARG_NAME: &str = "block-hash";
    const ARG_SHORT: &str = "b";
    const ARG_VALUE_NAME: &str = ARG_HEX_STRING;
    const ARG_HELP: &str = "Hex-encoded hash of the block";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::BlockHash as usize)
    }

    pub fn get<'a>(matches: &'a ArgMatches) -> Option<&'a str> {
        matches.value_of(ARG_NAME)
    }
}

/// Handles providing the arg for and retrieval of the key.
mod key {
    use casper_node::crypto::AsymmetricKeyExt;
    use casper_types::{AsymmetricType, PublicKey};

    use super::*;

    const ARG_NAME: &str = "key";
    const ARG_SHORT: &str = "k";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING or PATH";
    const ARG_HELP: &str =
        "The base key for the query. This must be a properly formatted public key, account hash, \
        contract address hash, URef, transfer hash, deploy-info hash,era-info number, bid, withdraw \
        or dictionary address. The format for each respectively is \"<HEX STRING>\", \
        \"account-hash-<HEX STRING>\", \"hash-<HEX STRING>\", \
        \"uref-<HEX STRING>-<THREE DIGIT INTEGER>\", \"transfer-<HEX-STRING>\", \
        \"deploy-<HEX-STRING>\", \"era-<u64>\", \"bid-<HEX-STRING>\",\
        \"withdraw-<HEX-STRING>\" or \"dictionary-<HEX-STRING>\". \
        The system contract registry key is unique and can only take the value: \
        system-contract-registry-0000000000000000000000000000000000000000000000000000000000000000. \
        \nThe public key may instead be read in from a file, in which case \
        enter the path to the file as the --key argument. The file should be one of the two public \
        key files generated via the `keygen` subcommand; \"public_key_hex\" or \"public_key.pem\"";

    pub(crate) fn arg(order: usize) -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(order)
    }

    pub(crate) fn get(matches: &ArgMatches) -> Result<String, Error> {
        let value = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));

        // Try to read as a PublicKey PEM file first.
        if let Ok(public_key) = PublicKey::from_file(value) {
            return Ok(public_key.to_hex());
        }

        // Try to read as a hex-encoded PublicKey file next.
        if let Ok(hex_public_key) = fs::read_to_string(value) {
            let _ = PublicKey::from_hex(&hex_public_key).map_err(|error| {
                eprintln!(
                    "Can't parse the contents of {} as a public key: {}",
                    value, error
                );
                Error::FailedToParseKey
            })?;
            return Ok(hex_public_key);
        }

        // Just return the value.
        Ok(value.to_string())
    }
}

/// Handles providing the arg for and retrieval of the key.
mod path {
    use super::*;

    const ARG_NAME: &str = "query-path";
    const ARG_SHORT: &str = "q";
    const ARG_VALUE_NAME: &str = "PATH/FROM/KEY";
    const ARG_HELP: &str = "The path from the key of the query";

    pub(crate) fn arg(order: usize) -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(order)
    }

    pub(crate) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches.value_of(ARG_NAME).unwrap_or_default()
    }
}

fn global_state_str_params<'a>(matches: &'a ArgMatches) -> GlobalStateStrParams<'a> {
    if let Some(state_root_hash) = state_root_hash::get(matches) {
        return GlobalStateStrParams {
            is_block_hash: false,
            hash_value: state_root_hash,
        };
    }
    if let Some(block_hash) = block_hash::get(matches) {
        return GlobalStateStrParams {
            is_block_hash: true,
            hash_value: block_hash,
        };
    }
    unreachable!("clap arg groups and parsing should prevent this for global state params")
}

#[async_trait]
impl<'a, 'b> ClientCommand<'a, 'b> for QueryGlobalState {
    const NAME: &'static str = "query-global-state";
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
            .arg(key::arg(DisplayOrder::Key as usize))
            .arg(path::arg(DisplayOrder::Path as usize))
            .arg(block_hash::arg())
            .arg(state_root_hash::arg())
            .group(
                ArgGroup::with_name("state-identifier")
                    .arg(state_root_hash::ARG_NAME)
                    .arg(block_hash::ARG_NAME)
                    .required(true),
            )
    }

    async fn run(matches: &ArgMatches<'a>) -> Result<Success, Error> {
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let verbosity_level = common::verbose::get(matches);
        let global_state_str_params = global_state_str_params(matches);
        let key = key::get(matches)?;
        let path = path::get(matches);

        casper_client::query_global_state(
            maybe_rpc_id,
            node_address,
            verbosity_level,
            global_state_str_params,
            &key,
            path,
        )
        .await
        .map(Success::from)
    }
}
