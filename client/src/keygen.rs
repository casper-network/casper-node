use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use clap::{App, Arg, ArgMatches, SubCommand};
use lazy_static::lazy_static;

use casper_node::crypto::asymmetric_key::{PublicKey, SecretKey};

use crate::{command::ClientCommand, common};

const PUBLIC_KEY_HEX: &str = "public_key_hex";
const SECRET_KEY_PEM: &str = "secret_key.pem";
const PUBLIC_KEY_PEM: &str = "public_key.pem";
const FILES: [&str; 3] = [PUBLIC_KEY_HEX, SECRET_KEY_PEM, PUBLIC_KEY_PEM];

lazy_static! {
    static ref MORE_ABOUT: String = format!(
        "{}. Creates {:?}. \"{}\" contains the hex-encoded key's bytes with the hex-encoded \
        algorithm tag prefixed",
        Keygen::ABOUT,
        FILES,
        PUBLIC_KEY_HEX
    );
}

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    OutputDir,
    Force,
    Algorithm,
}

/// Handles providing the arg for and retrieval of the output directory.
mod output_dir {
    use super::*;

    const ARG_NAME: &str = "output-dir";
    const ARG_VALUE_NAME: &str = common::ARG_PATH;
    const ARG_HELP: &str =
        "Path to output directory where key files will be created. If the path doesn't exist, it \
        will be created. If not set, the current working directory will be used";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::OutputDir as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> PathBuf {
        matches.value_of(ARG_NAME).unwrap_or_else(|| ".").into()
    }
}

/// Handles providing the arg for and retrieval of the key algorithm.
mod algorithm {
    use super::*;

    const ARG_NAME: &str = "algorithm";
    const ARG_SHORT: &str = "a";
    const ARG_VALUE_NAME: &str = common::ARG_STRING;
    pub(super) const ED25519: &str = "Ed25519";
    pub(super) const SECP256K1: &str = "secp256k1";
    const ARG_HELP: &str = "The type of keys to generate";

    pub fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .default_value(ED25519)
            .possible_value(ED25519)
            .possible_value(SECP256K1)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Algorithm as usize)
    }

    pub fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

pub struct Keygen {}

impl<'a, 'b> ClientCommand<'a, 'b> for Keygen {
    const NAME: &'static str = "keygen";
    const ABOUT: &'static str = "Generates account key files in the given directory";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .long_about(MORE_ABOUT.as_str())
            .display_order(display_order)
            .arg(output_dir::arg())
            .arg(common::force::arg(DisplayOrder::Force as usize, false))
            .arg(algorithm::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let output_dir = output_dir::get(matches);
        let force = common::force::get(matches);
        let algorithm = algorithm::get(matches);

        let _ = fs::create_dir_all(&output_dir)
            .unwrap_or_else(|error| panic!("should create {}: {}", output_dir.display(), error));
        let output_dir = output_dir.canonicalize().expect("should canonicalize path");

        if !force {
            for file in FILES.iter().map(|filename| output_dir.join(filename)) {
                if file.exists() {
                    eprintln!(
                        "{} exists. To overwrite, rerun with --{}",
                        file.display(),
                        common::force::ARG_NAME
                    );
                    process::exit(1);
                }
            }
        }

        let secret_key = if algorithm == algorithm::ED25519 {
            SecretKey::generate_ed25519()
        } else if algorithm == algorithm::SECP256K1 {
            SecretKey::generate_secp256k1()
        } else {
            panic!("Invalid key algorithm");
        };
        let public_key = PublicKey::from(&secret_key);

        write_file(PUBLIC_KEY_HEX, output_dir.as_path(), public_key.to_hex());

        let secret_key_path = output_dir.join(SECRET_KEY_PEM);
        secret_key
            .to_file(&secret_key_path)
            .unwrap_or_else(|error| {
                panic!("should write {}: {}", secret_key_path.display(), error)
            });

        let public_key_path = output_dir.join(PUBLIC_KEY_PEM);
        public_key
            .to_file(&public_key_path)
            .unwrap_or_else(|error| {
                panic!("should write {}: {}", public_key_path.display(), error)
            });

        println!("Wrote files to {}", output_dir.display());
    }
}

fn write_file(filename: &str, dir: &Path, value: String) {
    let path = dir.join(filename);
    fs::write(&path, value)
        .unwrap_or_else(|error| panic!("should write {}: {}", path.display(), error))
}
