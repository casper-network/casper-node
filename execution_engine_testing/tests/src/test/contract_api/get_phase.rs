use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, Phase};

const ARG_PHASE: &str = "phase";
const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_run_get_phase_contract() {
    let default_account = *DEFAULT_ACCOUNT_ADDR;

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_deploy_hash([1; 32])
            .with_session_code(
                "get_phase.wasm",
                runtime_args! { ARG_PHASE => Phase::Session },
            )
            .with_payment_code(
                "get_phase_payment.wasm",
                runtime_args! {
                    ARG_PHASE => Phase::Payment,
                    ARG_AMOUNT => *DEFAULT_PAYMENT
                },
            )
            .with_authorization_keys(&[default_account])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    LmdbWasmTestBuilder::default()
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit()
        .expect_success();
}
