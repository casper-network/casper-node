use casperlabs_engine_test_support::{
    internal::{ExecuteRequestBuilder, WasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_types::{account::AccountHash, runtime_args, RuntimeArgs, U512};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const TRANSFER_AMOUNT: u64 = 250_000_000 + 1000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);

#[ignore]
#[test]
fn should_run_add_bid() {
    let mut builder = WasmTestBuilder::default();

    let exec_request = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
    builder.exec(exec_request).commit().expect_success();

    let auction_hash = builder.get_auction_contract_hash();

    let auction_stored_value = builder
        .query(None, auction_hash.into(), &[])
        .expect("should query auction hash");
    let _auction = auction_stored_value
        .as_contract()
        .expect("should be contract");

    //
    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ADDR,
        auction_hash,
        "run_auction",
        runtime_args! {},
    )
    .build();

    builder.exec(exec_request).commit().expect_success();
}
