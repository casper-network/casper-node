use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_PATH,
};

use casper_types::{runtime_args, RuntimeArgs};

const ARG_AMOUNT: &str = "amount";
const EE_1225_REGRESSION_CONTRACT: &str = "ee_1225_regression.wasm";
const DO_NOTHING_CONTRACT: &str = "do_nothing.wasm";

#[should_panic(expected = "Finalization")]
#[ignore]
#[test]
fn should_run_ee_1225_verify_finalize_payment_invariants() {
    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder.run_genesis_with_default_genesis_accounts();
    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_payment_code(
                EE_1225_REGRESSION_CONTRACT,
                runtime_args! {
                    ARG_AMOUNT => *DEFAULT_PAYMENT,
                },
            )
            .with_session_code(DO_NOTHING_CONTRACT, RuntimeArgs::default())
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    builder.exec(exec_request).expect_success().commit();
}
