use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, RuntimeArgs, U512};

use rand::Rng;

const CONTRACT: &str = "regression_20240105.wasm";
#[ignore]
#[test]
fn should_observe_gas_limit_for_regression_20240105() {
    let session_args = runtime_args! {
        "collection_name" => "CEP78-TEST-1K",
        "collection_symbol" => "CEP781K",
        "total_token_supply" => 1_000_000_u64,
        "ownership_mode" => 2_u8,
        "nft_kind" => 1_u8,
        "nft_metadata_kind" => 0_u8,
        "json_schema" => "",
        "identifier_mode" => 0_u8,
        "metadata_mutability" => 0_u8,
    };

    let payment_args = runtime_args! { "amount" => U512::from(4_000_000_000_000u64) };
    let mut rng = rand::thread_rng();
    let deploy = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_session_code(CONTRACT, session_args)
        .with_empty_payment_bytes(payment_args.clone())
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash(rng.gen())
        .build();
    let exec_request = ExecuteRequestBuilder::from_deploy_item(deploy).build();

    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request);
    builder.expect_failure();
}
