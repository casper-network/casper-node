use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::shared::transform::Transform;
use casper_types::{runtime_args, RuntimeArgs, U512};

const CONTRACT_EE_460_REGRESSION: &str = "ee_460_regression.wasm";

const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_run_ee_460_no_side_effects_on_error_regression() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_460_REGRESSION,
        runtime_args! { ARG_AMOUNT => U512::max_value() },
    )
    .build();
    let result = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .finish();

    // In this regression test it is verified that no new urefs are created on the
    // mint uref, which should mean no new purses are created in case of
    // transfer error. This is considered sufficient cause to confirm that the
    // mint uref is left untouched.
    let mint_contract_uref = result.builder().get_mint_contract_hash();

    let transforms = &result.builder().get_transforms()[0];
    let mint_transforms = transforms
        .get(&mint_contract_uref.into())
        // Skips the Identity writes introduced since payment code execution for brevity of the
        // check
        .filter(|&v| v != &Transform::Identity);
    assert!(mint_transforms.is_none());
}
