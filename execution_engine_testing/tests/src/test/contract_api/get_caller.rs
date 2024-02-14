use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, runtime_args};

const CONTRACT_GET_CALLER: &str = "get_caller.wasm";
const CONTRACT_GET_CALLER_SUBCALL: &str = "get_caller_subcall.wasm";
const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);

#[ignore]
#[test]
fn should_run_get_caller_contract() {
    LmdbWasmTestBuilder::default()
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_GET_CALLER,
                runtime_args! {"account" => *DEFAULT_ACCOUNT_ADDR},
            )
            .build(),
        )
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_run_get_caller_contract_other_account() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
                runtime_args! {"target" => ACCOUNT_1_ADDR, "amount"=> *DEFAULT_PAYMENT},
            )
            .build(),
        )
        .expect_success()
        .commit();

    builder
        .exec(
            ExecuteRequestBuilder::standard(
                ACCOUNT_1_ADDR,
                CONTRACT_GET_CALLER,
                runtime_args! {"account" => ACCOUNT_1_ADDR},
            )
            .build(),
        )
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_run_get_caller_subcall_contract() {
    {
        let mut builder = LmdbWasmTestBuilder::default();
        builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

        builder
            .exec(
                ExecuteRequestBuilder::standard(
                    *DEFAULT_ACCOUNT_ADDR,
                    CONTRACT_GET_CALLER_SUBCALL,
                    runtime_args! {"account" => *DEFAULT_ACCOUNT_ADDR},
                )
                .build(),
            )
            .expect_success()
            .commit();
    }

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
                runtime_args! {"target" => ACCOUNT_1_ADDR, "amount"=>*DEFAULT_PAYMENT},
            )
            .build(),
        )
        .expect_success()
        .commit();
    builder
        .exec(
            ExecuteRequestBuilder::standard(
                ACCOUNT_1_ADDR,
                CONTRACT_GET_CALLER_SUBCALL,
                runtime_args! {"account" => ACCOUNT_1_ADDR},
            )
            .build(),
        )
        .expect_success()
        .commit();
}
