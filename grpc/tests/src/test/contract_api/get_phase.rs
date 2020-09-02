use casper_engine_test_support::{
    internal::{
        DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{runtime_args, Phase, RuntimeArgs};

const ARG_PHASE: &str = "phase";

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
                runtime_args! { ARG_PHASE => Phase::Payment },
            )
            .with_authorization_keys(&[default_account])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .expect_success();
}
