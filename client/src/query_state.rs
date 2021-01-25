use std::{fs, str};

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_node::rpcs::state::GetItem;
use casper_types::PublicKey;

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    StateRootHash,
    Key,
    Path,
}

/// Handles providing the arg for and retrieval of the key.
mod key {
    use casper_node::crypto::AsymmetricKeyExt;
    use casper_types::AsymmetricType;

    use super::*;

    const ARG_NAME: &str = "key";
    const ARG_SHORT: &str = "k";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING or PATH";
    const ARG_HELP: &str =
        "The base key for the query. This must be a properly formatted public key, account hash, \
        contract address hash, URef, transfer hash or deploy-info hash. The format for each \
        respectively is \"<HEX STRING>\", \"account-hash-<HEX STRING>\", \"hash-<HEX STRING>\", \
        \"uref-<HEX STRING>-<THREE DIGIT INTEGER>\", \"transfer-<HEX-STRING>\" and \
        \"deploy-<HEX-STRING>\". The public key may instead be read in from a file, in which case \
        enter the path to the file as the --key argument. The file should be one of the two public \
        key files generated via the `keygen` subcommand; \"public_key_hex\" or \"public_key.pem\"";

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
        let value = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));

        // Try to read as a PublicKey PEM file first.
        if let Ok(public_key) = PublicKey::from_file(value) {
            return public_key.to_hex();
        }

        // Try to read as a hex-encoded PublicKey file next.
        if let Ok(hex_public_key) = fs::read_to_string(value).map(|contents| {
            PublicKey::from_hex(&contents).unwrap_or_else(|error| {
                panic!(
                    "failed to parse '{}' as a hex-encoded public key file: {}",
                    value, error
                )
            });
            contents
        }) {
            return hex_public_key;
        }

        // Just return the value.
        value.to_string()
    }
}

/// Handles providing the arg for and retrieval of the key.
mod path {
    use super::*;

    const ARG_NAME: &str = "query-path";
    const ARG_SHORT: &str = "q";
    const ARG_VALUE_NAME: &str = "PATH/FROM/KEY";
    const ARG_HELP: &str = "The path from the key of the query";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Path as usize)
    }

    pub(super) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches.value_of(ARG_NAME).unwrap_or_default()
    }
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetItem {
    const NAME: &'static str = "query-state";
    const ABOUT: &'static str = "Retrieves a stored value from the network";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(common::state_root_hash::arg(
                DisplayOrder::StateRootHash as usize,
            ))
            .arg(key::arg())
            .arg(path::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let mut verbosity_level = common::verbose::get(matches);
        let state_root_hash = common::state_root_hash::get(matches);
        let key = key::get(matches);
        let path = path::get(matches);

        let response = casper_client::get_item(
            maybe_rpc_id,
            node_address,
            verbosity_level,
            state_root_hash,
            &key,
            path,
        )
        .unwrap_or_else(|error| panic!("response error: {}", error));

        if verbosity_level == 0 {
            verbosity_level += 1
        }
        casper_client::pretty_print_at_level(&response, verbosity_level);
    }
}
