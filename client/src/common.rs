use std::{env, fs, path::PathBuf};

use clap::{Arg, ArgMatches};

use casperlabs_node::crypto::asymmetric_key::SecretKey;

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
        let path = string_to_path_buf(
            matches
                .value_of(ARG_NAME)
                .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME)),
        );
        SecretKey::from_file(path).expect("should parse secret key pem file")
    }
}

pub fn string_to_path_buf(input: &str) -> PathBuf {
    let mut path = input.to_string();
    // Replace env vars in the provided path.
    for (env_var_name, env_var_value) in env::vars() {
        path = path.replace(&format!("${}", env_var_name), &env_var_value);
    }

    PathBuf::from(path)
}

pub fn read_file(path: &str) -> Vec<u8> {
    let path = string_to_path_buf(path);
    fs::read(&path).unwrap_or_else(|error| panic!("should read {}: {}", path.display(), error))
}
