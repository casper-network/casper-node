use crate::lmdb_fixture;
use casper_engine_test_support::{
    ExecuteRequestBuilder, UpgradeRequestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PROPOSER_ADDR,
    DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION, PRODUCTION_RUN_GENESIS_REQUEST,
};
use once_cell::sync::Lazy;
use std::collections::BTreeMap;

use casper_types::{
    runtime_args, CLValue, EraId, Key, ProtocolVersion, RuntimeArgs, StoredValue, URef,
};

const PURSE_FIXTURE: &str = "purse_fixture";
const PURSE_WASM: &str = "regression-20240726.wasm";

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});

#[ignore = "RUN_FIXTURE_GENERATORS env var should be enabled"]
#[test]
fn generate_20240726_fixture() {
    if !lmdb_fixture::is_fixture_generator_enabled() {
        return;
    }
    // To generate this fixture again you have to re-run this code release-1.4.13.
    let genesis_request = PRODUCTION_RUN_GENESIS_REQUEST.clone();
    lmdb_fixture::generate_fixture(PURSE_FIXTURE, genesis_request, |builder| {
        let proposer_account = builder
            .get_account(DEFAULT_PROPOSER_PUBLIC_KEY.to_account_hash())
            .expect("did not find the proposer account installed");
        let main_purse = proposer_account.main_purse();
        let exec_request = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            PURSE_WASM,
            runtime_args! {
                "uref" => main_purse.to_formatted_string()
            },
        )
        .build();
        builder.exec(exec_request).expect_success().commit();
    })
    .unwrap();
}

#[ignore]
#[test]
fn should_not_allow_forged_urefs_to_be_saved_to_named_keys() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(PURSE_FIXTURE);

    let contract_to_prune = *builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default account")
        .named_keys()
        .get("package-hash")
        .expect("must have package hash");

    let global_state_update = {
        let mut ret = BTreeMap::new();
        ret.insert(contract_to_prune, StoredValue::CLValue(CLValue::unit()));
        ret
    };

    let mut upgrade_config = UpgradeRequestBuilder::new()
        .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
        .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
        .with_activation_point(EraId::new(0))
        .with_global_state_update(global_state_update)
        .build();

    builder
        .upgrade_with_upgrade_request_and_config(None, &mut upgrade_config)
        .expect_upgrade_success()
        .commit();

    let contract_package = builder
        .query(None, contract_to_prune, &[])
        .expect("must get stored value")
        .as_contract_package()
        .cloned()
        .expect("must get package");

    for hash in contract_package.versions().values() {
        let contract = builder
            .query(None, Key::Hash(hash.value()), &[])
            .expect("must get stored value")
            .as_contract()
            .cloned()
            .expect("must get contract");
        assert!(contract.named_keys().is_empty());
    }

    let hardcoded_uref = builder
        .get_account(*DEFAULT_PROPOSER_ADDR)
        .expect("must get account")
        .main_purse();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        PURSE_WASM,
        runtime_args! {
            "uref" => hardcoded_uref.to_formatted_string()
        },
    )
    .build();

    builder.exec(exec_request).expect_failure();

    let error = builder.get_error().expect("must have error");
    let uref_str = hardcoded_uref.to_formatted_string();
    let hardcoded_main_purse = URef::from_formatted_str(&uref_str).unwrap();
    assert!(matches!(
        error,
        casper_execution_engine::core::engine_state::Error::Exec(
            casper_execution_engine::core::execution::Error::ForgedReference(forged_uref)
        )
        if forged_uref == hardcoded_main_purse
    ))
}
