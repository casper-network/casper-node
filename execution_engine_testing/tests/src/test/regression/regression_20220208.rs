use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{engine_state::Error, execution::Error as ExecError};
use casper_types::{account::AccountHash, runtime_args, system::mint, ApiError, RuntimeArgs, U512};

const REGRESSION_20220208_CONTRACT: &str = "regression_20220208.wasm";
const ARG_AMOUNT_PART_1: &str = "amount_part_1";
const ARG_AMOUNT_PART_2: &str = "amount_part_2";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([111; 32]);

const UNAPPROVED_SPENDING_AMOUNT_ERR: Error = Error::Exec(ExecError::Revert(ApiError::Mint(
    mint::Error::UnapprovedSpendingAmount as u8,
)));

#[ignore]
#[test]
fn should_transfer_within_approved_limit_multiple_transfers() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let part_1 = U512::from(100u64);
    let part_2 = U512::from(100u64);
    let transfers_limit = part_1 + part_2;

    let args = runtime_args! {
        ARG_AMOUNT_PART_1 => part_1,
        ARG_AMOUNT_PART_2 => part_2,
        mint::ARG_AMOUNT => transfers_limit,
        mint::ARG_TARGET => ACCOUNT_1_ADDR,
    };

    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, REGRESSION_20220208_CONTRACT, args)
            .build();

    builder.exec(exec_request).expect_success();
}

#[ignore]
#[test]
fn should_not_transfer_above_approved_limit_multiple_transfers() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let part_1 = U512::from(100u64);
    let part_2 = U512::from(100u64);
    let transfers_limit = part_1 + part_2 - U512::one();

    let args = runtime_args! {
        ARG_AMOUNT_PART_1 => part_1,
        ARG_AMOUNT_PART_2 => part_2,
        mint::ARG_AMOUNT => transfers_limit,
        mint::ARG_TARGET => ACCOUNT_1_ADDR,
    };

    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, REGRESSION_20220208_CONTRACT, args)
            .build();

    builder
        .exec(exec_request)
        .assert_error(UNAPPROVED_SPENDING_AMOUNT_ERR);
}
