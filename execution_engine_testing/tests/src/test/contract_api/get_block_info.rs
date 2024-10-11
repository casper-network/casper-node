use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_types::{bytesrepr::ToBytes, runtime_args, BlockHash};

const CONTRACT_GET_BLOCKINFO: &str = "get_blockinfo.wasm";
const ARG_FIELD_IDX: &str = "field_idx";

const FIELD_IDX_BLOCK_TIME: u8 = 0;
const ARG_KNOWN_BLOCK_TIME: &str = "known_block_time";

#[ignore]
#[test]
fn should_run_get_block_time() {
    let block_time: u64 = 42;

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GET_BLOCKINFO,
        runtime_args! {
            ARG_FIELD_IDX => FIELD_IDX_BLOCK_TIME,
            ARG_KNOWN_BLOCK_TIME => block_time
        },
    )
    .with_block_time(block_time)
    .build();
    LmdbWasmTestBuilder::default()
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit()
        .expect_success();
}

const FIELD_IDX_BLOCK_HEIGHT: u8 = 1;
const ARG_KNOWN_BLOCK_HEIGHT: &str = "known_block_height";

#[ignore]
#[test]
fn should_run_get_block_height() {
    let block_height: u64 = 1;

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GET_BLOCKINFO,
        runtime_args! {
            ARG_FIELD_IDX => FIELD_IDX_BLOCK_HEIGHT,
            ARG_KNOWN_BLOCK_HEIGHT => block_height
        },
    )
    .with_block_height(block_height)
    .build();
    LmdbWasmTestBuilder::default()
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .expect_success()
        .commit();
}

const FIELD_IDX_PARENT_BLOCK_HASH: u8 = 2;
const ARG_KNOWN_BLOCK_PARENT_HASH: &str = "known_block_parent_hash";

#[ignore]
#[test]
fn should_run_get_block_parent_hash() {
    let block_hash = BlockHash::default();
    let digest = block_hash.inner();
    let digest_bytes = digest.to_bytes().expect("should serialize");
    let bytes = casper_types::bytesrepr::Bytes::from(digest_bytes);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GET_BLOCKINFO,
        runtime_args! {
            ARG_FIELD_IDX => FIELD_IDX_PARENT_BLOCK_HASH,
            ARG_KNOWN_BLOCK_PARENT_HASH => bytes
        },
    )
    .with_parent_block_hash(block_hash)
    .build();
    LmdbWasmTestBuilder::default()
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .expect_success()
        .commit();
}

const FIELD_IDX_STATE_HASH: u8 = 3;
const ARG_KNOWN_STATE_HASH: &str = "known_state_hash";

#[ignore]
#[test]
fn should_run_get_state_hash() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let state_hash = builder.get_post_state_hash();
    let digest_bytes = state_hash.to_bytes().expect("should serialize");
    let bytes = casper_types::bytesrepr::Bytes::from(digest_bytes);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GET_BLOCKINFO,
        runtime_args! {
            ARG_FIELD_IDX => FIELD_IDX_STATE_HASH,
            ARG_KNOWN_STATE_HASH => bytes
        },
    )
    .with_state_hash(state_hash)
    .build();

    builder.exec(exec_request).expect_success().commit();
}
