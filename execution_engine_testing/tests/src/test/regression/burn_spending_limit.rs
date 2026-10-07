use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT,
    LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution::ExecError};
use casper_types::{
    api_error::ApiError,
    runtime_args,
    system::mint::{self, ARG_AMOUNT},
    U512,
};

const CONTRACT_BURN: &str = "burn_main_purse.wasm";

/// Regression for audit-confirmed-83: mint `burn` must enforce the approved spending limit when
/// the source purse is the caller's main purse, mirroring `Mint::transfer`. The session's outer
/// `amount` runtime arg becomes the transaction's approved spending limit; we set it small and
/// then ask the contract to burn a much larger inner amount from the main purse. Without the fix
/// the burn succeeds because `Mint::burn` skips the spending-limit check entirely.
#[ignore]
#[test]
fn mint_burn_from_main_purse_must_enforce_spending_limit() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let outer_spending_limit = U512::one();
    let burn_amount = *DEFAULT_PAYMENT * U512::from(10u64);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_BURN,
        runtime_args! {
            ARG_AMOUNT => outer_spending_limit,
            "burn_amount" => burn_amount,
        },
    )
    .build();

    builder.exec(exec_request).commit();

    let error = builder
        .get_error()
        .expect("burn beyond approved spending limit should error");
    assert!(
        matches!(
            error,
            Error::Exec(ExecError::Revert(ApiError::Mint(code)))
            if code == mint::Error::UnapprovedSpendingAmount as u8
        ),
        "mint burn bypassed the approved spending limit: {error:?}",
    );
}
