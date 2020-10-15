use std::{fs, str};

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_node::{crypto::asymmetric_key::PublicKey, rpcs::state::GetItem};
use casper_types::Key;

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    GlobalStateHash,
    Key,
    Path,
}

/// Handles providing the arg for and retrieval of the key.
mod key {
    use super::*;

    const ARG_NAME: &str = "key";
    const ARG_SHORT: &str = "k";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING or PATH";
    const ARG_HELP: &str =
        "The base key for the query.  This must be a properly formatted public key, account hash, \
        contract address hash or URef.  The format for each respectively is \"<HEX STRING>\", \
        \"account-hash-<HEX STRING>\", \"hash-<HEX STRING>\" and \
        \"uref-<HEX STRING>-<THREE DIGIT INTEGER>\".  The public key may instead be read in from a \
        file, in which case enter the path to the file as the --key argument.  The file should be \
        one of the two public key files generated via the `keygen` subcommand; \"public_key_hex\" \
        or \"public_key.pem\"";

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

        // Try to parse as a `Key` first.
        if Key::from_formatted_str(value).is_ok() {
            return value.to_string();
        }

        // Try to parse from a hex-encoded `PublicKey`, a pem-encoded file then a hex-encoded file.
        let public_key = if let Ok(public_key) = PublicKey::from_hex(value) {
            public_key
        } else if let Ok(public_key) = PublicKey::from_file(value) {
            public_key
        } else {
            let contents = fs::read(value).unwrap_or_else(|_| {
                panic!(
                    "failed to parse '{}' as a public key (as a hex string, hex file or pem file), \
                    account hash, contract address hash or URef",
                    value
                )
            });
            PublicKey::from_hex(contents).unwrap_or_else(|error| {
                panic!(
                    "failed to parse '{}' as a hex-encoded public key file: {}",
                    value, error
                )
            })
        };

        // Return the public key as an account hash.
        public_key.to_account_hash().to_formatted_string()
    }
}

/// Handles providing the arg for and retrieval of the key.
mod path {
    use super::*;

    const ARG_NAME: &str = "query-path";
    const ARG_SHORT: &str = "q";
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

impl<'a, 'b> ClientCommand<'a, 'b> for GetItem {
    const NAME: &'static str = "query-state";
    const ABOUT: &'static str = "Retrieves a stored value from global state";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::global_state_hash::arg(
                DisplayOrder::GlobalStateHash as usize,
            ))
            .arg(key::arg())
            .arg(path::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let global_state_hash = common::global_state_hash::get(matches);
        let key = key::get(matches);
        let path = path::get(matches);

        let response_value =
            casper_client::query_state::get_item(node_address, global_state_hash, key, path)
                .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{:?}", response_value);
    }
}
