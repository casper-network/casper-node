mod bids;
mod distribute;

use casper_engine_test_support::internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder};
use casper_types::{
    self, account::AccountHash, auction::METHOD_RUN_AUCTION, runtime_args, RuntimeArgs,
};

const ARG_ENTRY_POINT: &str = "entry_point";
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);
const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";

fn run_auction(builder: &mut InMemoryWasmTestBuilder) {
    let run_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_RUN_AUCTION
        },
    )
    .build();
    builder.exec(run_request).commit().expect_success();
}
