use std::{fs, path::PathBuf};

use clap::{Arg, ArgMatches};

use casper_node::crypto::asymmetric_key::SecretKey;

/// The node HTTP endpoint to instruct it to put the provided deploy.
pub const DEPLOY_API_PATH: &str = "deploys";

/// Handles providing the arg for and retrieval of the node hostname/IP and port.
pub mod node_address {
    use super::*;

    const ARG_NAME: &str = "node-address";
    const ARG_SHORT: &str = "n";
    const ARG_VALUE_NAME: &str = "HOST:PORT";
    const ARG_DEFAULT: &str = "http://localhost:7777";
    const ARG_HELP: &str = "Hostname or IP and port of node on which HTTP service is running";

    pub fn arg(order: usize) -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .default_value(ARG_DEFAULT)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(order)
    }

    pub fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

/// Handles providing the arg for and retrieval of the secret key.
pub mod secret_key {
    use super::*;

    const ARG_NAME: &str = "secret-key";
    const ARG_VALUE_NAME: &str = "PATH";
    const ARG_HELP: &str = "Path to secret key file";

    pub fn arg(order: usize) -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(order)
    }

    pub fn get(matches: &ArgMatches) -> SecretKey {
        let path = PathBuf::from(
            matches
                .value_of(ARG_NAME)
                .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME)),
        );
        SecretKey::from_file(path).expect("should parse secret key pem file")
    }
}

/// Handles the arg for whether to overwrite existing output file(s).
pub mod force {
    use super::*;

    pub const ARG_NAME: &str = "force";
    const ARG_NAME_SHORT: &str = "f";
    const ARG_HELP_SINGULAR: &str =
        "If this flag is passed and the output file already exists, it will be overwritten. \
        Without this flag, if the output file already exists, the command will fail";
    const ARG_HELP_PLURAL: &str =
        "If this flag is passed, any existing output files will be overwritten. Without this flag, \
        if any output file exists, no output files will be generated and the command will fail";

    pub fn arg(order: usize, singular: bool) -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .short(ARG_NAME_SHORT)
            .required(false)
            .help(if singular {
                ARG_HELP_SINGULAR
            } else {
                ARG_HELP_PLURAL
            })
            .display_order(order)
    }

    pub fn get(matches: &ArgMatches) -> bool {
        matches.is_present(ARG_NAME)
    }
}

/// Handles providing the arg for and retrieval of the global state hash.
pub mod global_state_hash {
    use super::*;

    const ARG_NAME: &str = "global-state-hash";
    const ARG_SHORT: &str = "g";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str = "Hex-encoded global state hash";

    pub(crate) fn arg(order: usize) -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(order)
    }

    pub(crate) fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

/// Handles providing the arg for and retrieval of the block hash.
pub mod block_hash {
    use super::*;

    const ARG_NAME: &str = "block-hash";
    const ARG_SHORT: &str = "b";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str =
        "Hex-encoded block hash.  If not given, the latest finalized block as known at the given \
        node will be used";

    pub(crate) fn arg(order: usize) -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(order)
    }

    pub(crate) fn get(matches: &ArgMatches) -> Option<String> {
        matches.value_of(ARG_NAME).map(ToString::to_string)
    }
}

pub fn read_file(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("should read {}: {}", path, error))
}
