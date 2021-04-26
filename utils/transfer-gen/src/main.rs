use std::convert::{TryFrom, TryInto};

use clap::{crate_version, App, Arg};

use casper_engine_test_support::internal::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder,
};
use casper_execution_engine::shared::{
    newtypes::Blake2bHash, stored_value::StoredValue, transform::Transform,
};
use casper_types::{
    account::{AccountHash, AccountHashBytes},
    bytesrepr::ToBytes,
    runtime_args,
    system::mint,
    AsymmetricType, Key, PublicKey, RuntimeArgs, U512,
};

fn str_to_account_hash(s: &str) -> AccountHash {
    let bytes = AccountHashBytes::try_from(base16::decode(s).unwrap().as_ref()).unwrap();
    AccountHash::new(bytes)
}

/// Parses a Blake2bHash from a string. Panics if parsing fails.
pub fn hash_from_str(hex_str: &str) -> Blake2bHash {
    (&base16::decode(hex_str).unwrap()[..]).try_into().unwrap()
}

/// Prints a global state update entry in a format ready for inclusion in a TOML file.
fn print_entry(key: &Key, value: &StoredValue) {
    println!("[[entries]]");
    println!("key = \"{}\"", key.to_formatted_string());
    println!("value = \"{}\"", base64::encode(value.to_bytes().unwrap()));
    println!();
}

fn main() {
    let matches = App::new("Global State Update Transfer Generator")
        .version(crate_version!())
        .about("Generates a global state update file based on the supplied parameters")
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
                .help("Source account hash")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("to")
                .short("t")
                .long("to")
                .value_name("ACCOUNT_HASH")
                .help("Target account hash")
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
        )
        .arg(
            Arg::with_name("proposer")
                .short("p")
                .long("proposer")
                .value_name("PUBLIC_KEY")
                .help("Public key of the proposer")
                .takes_value(true)
                .required(true),
        )
        .get_matches();

    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = matches.value_of("hash").unwrap();

    let from_account = str_to_account_hash(&matches.value_of("from").unwrap());
    let to_account = str_to_account_hash(&matches.value_of("to").unwrap());
    let amount = U512::from_str_radix(&matches.value_of("amount").unwrap(), 10).unwrap();
    let proposer = PublicKey::from_hex(matches.value_of("proposer").unwrap().as_bytes()).unwrap();

    // Open the global state that should be in the supplied directory.
    let mut builder =
        LmdbWasmTestBuilder::open_raw(data_dir, Default::default(), hash_from_str(state_hash));

    let no_wasm_transfer_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(from_account)
            .with_empty_payment_bytes(runtime_args! {})
            .with_transfer_args(runtime_args! {
                mint::ARG_TARGET => Key::Account(to_account),
                mint::ARG_AMOUNT => amount,
                mint::ARG_ID => Option::<u64>::None,   // TODO: Do we want some ID here?
            })
            .with_authorization_keys(&[from_account])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy_item)
            .with_proposer(proposer)
            .build()
    };

    builder.exec(no_wasm_transfer_request).expect_success();

    let transforms = builder.get_transforms();

    for (key, value) in &transforms[0] {
        match value {
            Transform::Write(val) => print_entry(key, val),
            _ => (),
        }
    }
}
