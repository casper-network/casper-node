use casper_engine_test_support::{
    ChainspecConfig, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION,
};
use casper_execution_engine::{engine_state::Error, execution::ExecError};
use casper_types::{
    runtime_args,
    system::auction::{ARG_AMOUNT, ARG_DELEGATION_RATE, ARG_PUBLIC_KEY},
    ApiError, U512,
};
use once_cell::sync::Lazy;
use std::path::PathBuf;

const ADD_BIDS_WASM: &str = "auction_bids.wasm";
const ARG_ENTRY_POINT: &str = "entry_point";
/// The name of the chainspec file on disk.
pub const CHAINSPEC_NAME: &str = "chainspec.toml";
pub static LOCAL_PATH: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/local/"));

#[ignore]
#[test]
fn add_auction_should_fail_when_delegation_rate_not_met() {
    let path = LOCAL_PATH.join(CHAINSPEC_NAME);
    let mut chainspec =
        ChainspecConfig::from_chainspec_path(path).expect("must build chainspec configuration");
    chainspec = chainspec.with_minimum_delegation_rate(20);
    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(chainspec.clone());
    let genesis_request = chainspec
        .create_genesis_request(DEFAULT_ACCOUNTS.clone(), DEFAULT_PROTOCOL_VERSION)
        .unwrap();
    builder.run_genesis(genesis_request);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        ADD_BIDS_WASM,
        runtime_args! {
            ARG_ENTRY_POINT => "add_bid",
            ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            ARG_AMOUNT => U512::from(10_000_000_000_000u64),
            ARG_DELEGATION_RATE => 19u8,
        },
    )
    .build();

    let commit = builder.exec(exec_request).commit();
    commit.expect_failure();
    let last_exec_result = commit
        .get_last_exec_result()
        .expect("Expected to be called after exec()");
    assert!(matches!(
        last_exec_result.error().cloned(),
        Some(Error::Exec(ExecError::Revert(ApiError::AuctionError(64))))
    ));
}
