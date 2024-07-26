use crate::lmdb_fixture;
use casper_engine_test_support::{
    ExecuteRequestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PROPOSER_PUBLIC_KEY,
    PRODUCTION_RUN_GENESIS_REQUEST,
};

use casper_types::{runtime_args, RuntimeArgs};

const PURSE_FIXTURE: &str = "purse_fixture";
const PURSE_WASM: &str = "regression-20240726.wasm";
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
        println!("{:?}", main_purse.to_formatted_string());
    })
    .unwrap();
}

#[ignore]
#[test]
fn should_not_allow_forged_urefs_to_be_saved_to_named_keys() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(PURSE_FIXTURE);

    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, PURSE_WASM, runtime_args! {})
            .build();

    builder.exec(exec_request).expect_failure();

    let error = builder.get_error().expect("must have error");
    assert!(matches!(
        error,
        casper_execution_engine::core::engine_state::Error::Exec(
            casper_execution_engine::core::execution::Error::ForgedReference(forged_uref) if forged_uref == hardcoded_main_purse
        )
    ))
}
