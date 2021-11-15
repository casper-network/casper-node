use std::{env, path::PathBuf, str::FromStr};

use clap::{Arg, ArgMatches};

use casper_engine_test_support::MINIMUM_ACCOUNT_CREATION_BALANCE;
use casper_types::{account::AccountHash, U512};

const DATA_DIR_ARG_NAME: &str = "data-dir";
const DATA_DIR_ARG_SHORT: &str = "d";
const DATA_DIR_ARG_LONG: &str = "data-dir";
const DATA_DIR_ARG_VALUE_NAME: &str = "PATH";
const DATA_DIR_ARG_HELP: &str = "Directory in which persistent data is stored [default: current \
                                 working directory]";

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
pub const ACCOUNT_1_INITIAL_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_2_ADDR: AccountHash = AccountHash::new([2u8; 32]);

pub enum TransferMode {
    WASM,
    WASMLESS,
}

pub fn exe_name() -> String {
    env::current_exe()
        .expect("Expected to read current executable's name")
        .file_stem()
        .expect("Expected a file name for the current executable")
        .to_str()
        .expect("Expected valid unicode for the current executable's name")
        .to_string()
}

pub fn data_dir_arg() -> Arg<'static, 'static> {
    Arg::with_name(DATA_DIR_ARG_NAME)
        .short(DATA_DIR_ARG_SHORT)
        .long(DATA_DIR_ARG_LONG)
        .value_name(DATA_DIR_ARG_VALUE_NAME)
        .help(DATA_DIR_ARG_HELP)
        .takes_value(true)
}

pub fn data_dir(arg_matches: &ArgMatches) -> PathBuf {
    match arg_matches.value_of(DATA_DIR_ARG_NAME) {
        Some(dir) => PathBuf::from_str(dir).expect("Expected a valid unicode path"),
        None => env::current_dir().expect("Expected to be able to access current working dir"),
    }
}

pub fn parse_hash(encoded_hash: &str) -> Vec<u8> {
    base16::decode(encoded_hash).expect("Expected a valid, hex-encoded hash")
}

pub fn parse_count(count_as_str: &str) -> usize {
    let count: usize = count_as_str.parse().expect("Expected an integral count");
    assert!(count > 0, "Expected count > 0");
    count
}

pub fn parse_transfer_mode(transfer_mode: &str) -> TransferMode {
    match transfer_mode {
        "WASM" => TransferMode::WASM,
        _ => TransferMode::WASMLESS,
    }
}

pub fn account_1_account_hash() -> AccountHash {
    ACCOUNT_1_ADDR
}

pub fn account_1_initial_amount() -> U512 {
    ACCOUNT_1_INITIAL_AMOUNT.into()
}

pub fn account_2_account_hash() -> AccountHash {
    ACCOUNT_2_ADDR
}
