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
mod key {
    use casper_node::crypto::AsymmetricKeyExt;
    use casper_types::AsymmetricType;

    use super::*;

    const ARG_NAME: &str = "public-key";
    const ARG_SHORT: &str = "pk";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING or PATH";
    const ARG_HELP: &str = "Path to the public key or the formatted hex string";

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
    const ABOUT: &'static str = "Generate an account hash from a given public key";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(key::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let mut verbosity_level = common::verbose::get(matches);
        let key_string = key::get(matches);
        let public_key = PublicKey::from_hex(key_string)
            .unwrap_or_else(|error| panic!("error in retrieving public key: {}", error));
        let account_hash = public_key.to_account_hash();

        if verbosity_level == 0 {
            verbosity_level += 1
        }

        casper_client::pretty_print_at_level(&account_hash, verbosity_level)
    }
}
