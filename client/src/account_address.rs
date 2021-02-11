use std::{fs, str};

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_types::{AsymmetricType, PublicKey};

use crate::{command::ClientCommand, common};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    Key,
}

/// Handles providing the arg for and retrieval of the key.
mod public_key {
    use casper_node::crypto::AsymmetricKeyExt;
    use casper_types::AsymmetricType;

    use super::*;

    const ARG_NAME: &str = "public-key";
    const ARG_SHORT: &str = "p";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING or PATH";
    const ARG_HELP: &str = "This must be a properly formatted public key. The public key may instead be read in from a file, in which case \
        enter the path to the file as the --public-key argument. The file should be one of the two public \
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

        if let Ok(public_key) = PublicKey::from_file(value) {
            return public_key.to_hex();
        }

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

        value.to_string()
    }
}

pub struct GenerateAccountHash {}

impl<'a, 'b> ClientCommand<'a, 'b> for GenerateAccountHash {
    const NAME: &'static str = "account-address";
    const ABOUT: &'static str = "Generates an account hash from a given public key";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(public_key::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let key_string = public_key::get(matches);
        let public_key = PublicKey::from_hex(key_string)
            .unwrap_or_else(|error| panic!("error in retrieving public key: {}", error));
        let account_hash = public_key.to_account_hash();

        println!("Account hash is [{}]", account_hash);
    }
}
