use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    addressable_entity::EntityKindTag, execution::TransformKind, runtime_args, Key, U512,
};

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
    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request_1)
        .expect_success()
        .commit();

    // In this regression test it is verified that no new urefs are created on the
    // mint uref, which should mean no new purses are created in case of
    // transfer error. This is considered sufficient cause to confirm that the
    // mint uref is left untouched.
    let mint_entity_key =
        Key::addressable_entity_key(EntityKindTag::System, builder.get_mint_contract_hash());

    let effects = &builder.get_effects()[0];
    let mint_transforms = effects
        .transforms()
        .iter()
        .find(|transform| transform.key() == &mint_entity_key)
        // Skips the Identity writes introduced since payment code execution for brevity of the
        // check
        .filter(|transform| transform.kind() != &TransformKind::Identity);
    assert!(mint_transforms.is_none());
}
