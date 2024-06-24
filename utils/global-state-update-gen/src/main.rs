mod admins;
mod balances;
mod decode;
mod generic;
mod system_entity_registry;
mod utils;
mod validators;

use admins::generate_admins;
use clap::{crate_version, App, Arg, SubCommand};

use crate::{
    balances::generate_balances_update, decode::decode_file, generic::generate_generic_update,
    system_entity_registry::generate_system_entity_registry,
    validators::generate_validators_update,
};

fn main() {
    let matches = App::new("Global State Update Generator")
        .version(crate_version!())
        .about("Generates a global state update file based on the supplied parameters")
        .subcommand(
            SubCommand::with_name("change-validators")
                .about("Generates an update changing the validators set")
                .arg(
                    Arg::with_name("data_dir")
                        .short("d")
                        .long("data-dir")
                        .value_name("PATH")
                        .help("Data storage directory containing the global state database file")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("hash")
                        .short("s")
                        .long("state-hash")
                        .value_name("HEX_STRING")
                        .help("The global state hash to be used as the base")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("validator")
                        .short("v")
                        .long("validator")
                        .value_name("KEY,STAKE[,BALANCE]")
                        .help("A validator config in the format 'public_key,stake[,balance]'")
                        .takes_value(true)
                        .required(true)
                        .multiple(true)
                        .number_of_values(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("balances")
                .about("Generates an update changing account balances")
                .arg(
                    Arg::with_name("data_dir")
                        .short("d")
                        .long("data-dir")
                        .value_name("PATH")
                        .help("Data storage directory containing the global state database file")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("hash")
                        .short("s")
                        .long("state-hash")
                        .value_name("HEX_STRING")
                        .help("The global state hash to be used as the base")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("from")
                        .short("f")
                        .long("from")
                        .value_name("ACCOUNT_HASH")
                        .help("Source account hash (with the account-hash- prefix)")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("to")
                        .short("t")
                        .long("to")
                        .value_name("ACCOUNT_HASH")
                        .help("Target account hash (with the account-hash- prefix)")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("amount")
                        .short("a")
                        .long("amount")
                        .value_name("MOTES")
                        .help("Amount to be transferred")
                        .takes_value(true)
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("migrate-into-system-contract-registry")
                .about("Generates an update creating the system entity registry")
                .arg(
                    Arg::with_name("data_dir")
                        .short("d")
                        .long("data-dir")
                        .value_name("PATH")
                        .help("Data storage directory containing the global state database file")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("hash")
                        .short("s")
                        .long("state-hash")
                        .value_name("HEX_STRING")
                        .help("The global state hash to be used as the base")
                        .takes_value(true)
                        .required(false),
                ),
        )
        .subcommand(
            SubCommand::with_name("generic")
                .about("Generates a generic update based on a config file")
                .arg(
                    Arg::with_name("data_dir")
                        .short("d")
                        .long("data-dir")
                        .value_name("PATH")
                        .help("Data storage directory containing the global state database file")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("hash")
                        .short("s")
                        .long("state-hash")
                        .value_name("HEX_STRING")
                        .help("The global state hash to be used as the base")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("config_file")
                        .value_name("FILE")
                        .index(1)
                        .required(true)
                        .help("The config file to be used for generating the update"),
                ),
        )
        .subcommand(
            SubCommand::with_name("generate-admins")
                .about("Generates entries to create new admin accounts on a private chain")
                .arg(
                    Arg::with_name("data_dir")
                        .short("d")
                        .long("data-dir")
                        .value_name("PATH")
                        .help("Data storage directory containing the global state database file")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("hash")
                        .short("s")
                        .long("state-hash")
                        .value_name("HEX_STRING")
                        .help("The global state hash to be used as the base")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("admin")
                        .short("a")
                        .long("admin")
                        .value_name("PUBLIC_KEY,BALANCE")
                        .help("A new admin account")
                        .takes_value(true)
                        .required(true)
                        .multiple(true)
                        .number_of_values(1),
                ),
        )
        .subcommand(
            SubCommand::with_name("decode")
                .about("Decodes the global_state.toml file into a readable form")
                .arg(
                    Arg::with_name("file")
                        .value_name("FILE")
                        .index(1)
                        .required(true)
                        .help("The file to be decoded"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        ("change-validators", Some(sub_matches)) => generate_validators_update(sub_matches),
        ("balances", Some(sub_matches)) => generate_balances_update(sub_matches),
        ("migrate-into-system-contract-registry", Some(sub_matches)) => {
            generate_system_entity_registry(sub_matches)
        }
        ("generic", Some(sub_matches)) => generate_generic_update(sub_matches),
        ("generate-admins", Some(sub_matches)) => generate_admins(sub_matches),
        ("decode", Some(sub_matches)) => decode_file(sub_matches),
        (subcommand, _) => {
            println!("Unknown subcommand: \"{}\"", subcommand);
        }
    }
}
