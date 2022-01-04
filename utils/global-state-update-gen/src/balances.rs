use clap::ArgMatches;

use casper_engine_test_support::{DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder};
use casper_execution_engine::shared::{additive_map::AdditiveMap, transform::Transform};
use casper_types::{
    account::AccountHash, runtime_args, system::mint, AsymmetricType, Key, PublicKey, RuntimeArgs,
    U512,
};

use crate::utils::{hash_from_str, print_entry};

pub(crate) fn generate_balances_update(matches: &ArgMatches<'_>) {
    let data_dir = matches.value_of("data_dir").unwrap_or(".");
    let state_hash = matches.value_of("hash").unwrap();

    let from_account = AccountHash::from_formatted_str(matches.value_of("from").unwrap()).unwrap();
    let to_account = AccountHash::from_formatted_str(matches.value_of("to").unwrap()).unwrap();
    let amount = U512::from_str_radix(matches.value_of("amount").unwrap(), 10).unwrap();
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

    // Combine the journal into an AdditiveMap in order to collapse Writes and Adds into just Writes
    let transforms = builder
        .get_execution_journals()
        .into_iter()
        .map(AdditiveMap::from)
        .next()
        .unwrap();

    for (key, value) in transforms {
        if let Transform::Write(val) = value {
            print_entry(&key, &val);
        }
    }
}
