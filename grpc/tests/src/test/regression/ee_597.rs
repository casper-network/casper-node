use casper_engine_test_support::{
    internal::{
        utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{system_contract_errors::auction, ApiError, RuntimeArgs};

const CONTRACT_EE_597_REGRESSION: &str = "ee_597_regression.wasm";

#[ignore]
#[test]
fn should_fail_when_bonding_amount_is_zero_ee_597_regression() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_597_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let result = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .finish();

    let response = result
        .builder()
        .get_exec_response(0)
        .expect("should have a response")
        .to_owned();

    let error_message = utils::get_error_message(response);

    // Error::BondTooSmall => 5,
    assert!(
        error_message.contains(&format!(
            "{:?}",
            ApiError::from(auction::Error::BondTooSmall)
        )),
        error_message
    );
}
