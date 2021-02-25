use std::{fs, str};

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_client::Error;
use casper_types::{AsymmetricType, PublicKey};

use crate::{command::ClientCommand, common, Success};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    Key,
}

/// Handles providing the arg for and retrieval of the public key.
mod public_key {
    use casper_node::crypto::AsymmetricKeyExt;
    use casper_types::AsymmetricType;

    use super::*;

    const ARG_NAME: &str = "public-key";
    const ARG_SHORT: &str = "p";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING or PATH";
    const ARG_HELP: &str =
        "This must be a properly formatted public key. The public key may instead be read in from \
        a file, in which case enter the path to the file as the --public-key argument. The file \
        should be one of the two public key files generated via the `keygen` subcommand; \
        \"public_key_hex\" or \"public_key.pem\"";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Key as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Result<String, Error> {
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

        Ok(value.to_string())
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

    fn run(matches: &ArgMatches<'_>) -> Result<Success, Error> {
        let hex_public_key = public_key::get(matches)?;
        let public_key = PublicKey::from_hex(&hex_public_key).map_err(|error| {
            eprintln!("Can't parse {} as a public key: {}", hex_public_key, error);
            Error::FailedToParseKey
        })?;
        let account_hash = public_key.to_account_hash();
        Ok(Success::Output(account_hash.to_string()))
    }
}
