use assert_matches::assert_matches;

use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::core::engine_state::QueryResult;
use casper_types::{bytesrepr::FromBytes, runtime_args, CLTyped, Key, RuntimeArgs, U512};

const CONTRACT_WRITE_DELETE: &str = "write_delete.wasm";
const CONTRACT_WRITE_DELETE_INLINE: &str = "write_delete_inline.wasm";

const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_NAME: &str = "name";
const ARG_VALUE: &str = "value";
const ARG_RESULT: &str = "result";

const METHOD_WRITE: &str = "write";
const METHOD_DELETE: &str = "delete";
const METHOD_READ: &str = "read";

fn do_read(builder: &mut InMemoryWasmTestBuilder, value_key: &str, result_key: &str) {
    let read_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_WRITE_DELETE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_READ,
            ARG_NAME => value_key,
            ARG_RESULT => result_key,
        },
    )
    .build();

    builder.exec(read_request).commit().expect_success();
}

fn do_write(builder: &mut InMemoryWasmTestBuilder, value_key: &str, value: U512) {
    let write_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_WRITE_DELETE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_WRITE,
            ARG_NAME => value_key,
            ARG_VALUE => value,
        },
    )
    .build();
    builder.exec(write_request).commit().expect_success();
}

fn do_delete(builder: &mut InMemoryWasmTestBuilder, value_key: &str) {
    let delete_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_WRITE_DELETE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DELETE,
            ARG_NAME => value_key,
        },
    )
    .build();
    builder.exec(delete_request).commit().expect_success();
}

fn query<T>(builder: &mut InMemoryWasmTestBuilder, value_key: &str) -> T
where
    T: FromBytes + CLTyped,
{
    builder
        .query(
            None,
            Key::Account(*DEFAULT_ACCOUNT_ADDR),
            &[value_key.to_string()],
        )
        .expect("should have value")
        .as_cl_value()
        .expect("should be CLValue")
        .clone()
        .into_t::<T>()
        .unwrap()
}

fn query_result(builder: &mut InMemoryWasmTestBuilder, value_key: &str) -> QueryResult {
    builder.query_result(
        None,
        Key::Account(*DEFAULT_ACCOUNT_ADDR),
        &[value_key.to_string()],
    )
}

#[ignore]
#[test]
fn write_delete() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let value_key = "one";
    let result_key = "result";
    let value = U512::one();

    do_write(&mut builder, value_key, value);
    let actual_value: U512 = query(&mut builder, value_key);
    assert_eq!(actual_value, value);

    do_read(&mut builder, value_key, result_key);
    let maybe_value: Option<U512> = query(&mut builder, result_key);
    assert_eq!(maybe_value, Some(U512::one()), "should be Some");

    do_delete(&mut builder, value_key);

    let query_result = query_result(&mut builder, value_key);
    assert_matches!(query_result, QueryResult::ValueNotFound(_));

    do_read(&mut builder, value_key, result_key);
    let query_result: Option<U512> = query(&mut builder, result_key);
    assert_eq!(query_result, None, "should be None");
}

#[ignore]
#[test]
fn write_delete_delete() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let value_key = "one";
    let value = U512::one();

    do_write(&mut builder, value_key, value);
    let actual_value: U512 = query(&mut builder, value_key);
    assert_eq!(actual_value, value);

    do_delete(&mut builder, value_key);
    let result = query_result(&mut builder, value_key);
    assert_matches!(result, QueryResult::ValueNotFound(_));

    do_delete(&mut builder, value_key);
    let result = query_result(&mut builder, value_key);
    assert_matches!(result, QueryResult::ValueNotFound(_));
}

#[ignore]
#[test]
fn write_delete_inline() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let write_delete_inline_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_WRITE_DELETE_INLINE,
        RuntimeArgs::new(),
    )
    .build();

    builder
        .exec(write_delete_inline_request)
        .commit()
        .expect_success();
}
