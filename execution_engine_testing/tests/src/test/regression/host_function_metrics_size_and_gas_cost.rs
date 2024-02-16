use std::convert::TryInto;

use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state::{self, ExecuteRequest},
    execution,
};
use casper_types::{
    account::{AccountHash, ACCOUNT_HASH_LENGTH},
    bytesrepr::Bytes,
    runtime_args,
    system::standard_payment,
    ApiError, U512,
};

const CONTRACT_HOST_FUNCTION_METRICS: &str = "host_function_metrics.wasm";
const CONTRACT_TRANSFER_TO_ACCOUNT_U512: &str = "transfer_to_account_u512.wasm";

// This value is not systemic, as code is added the size of WASM will increase,
// you can change this value to reflect the increase in WASM size.
const HOST_FUNCTION_METRICS_STANDARD_SIZE: usize = 116_188;
const HOST_FUNCTION_METRICS_STANDARD_GAS_COST: u64 = 422_402_224_490;

/// Acceptable size regression/improvement in percentage.
const SIZE_MARGIN: usize = 5;
/// Acceptable gas cost regression/improvement in percentage.
const GAS_COST_MARGIN: u64 = 5;

const HOST_FUNCTION_METRICS_MAX_SIZE: usize =
    HOST_FUNCTION_METRICS_STANDARD_SIZE * (100 + SIZE_MARGIN) / 100;
const HOST_FUNCTION_METRICS_MAX_GAS_COST: u64 =
    HOST_FUNCTION_METRICS_STANDARD_GAS_COST * (100 + GAS_COST_MARGIN) / 100;

const ACCOUNT0_ADDR: AccountHash = AccountHash::new([42; ACCOUNT_HASH_LENGTH]);
const ACCOUNT1_ADDR: AccountHash = AccountHash::new([43; ACCOUNT_HASH_LENGTH]);

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

const ARG_SEED: &str = "seed";
const ARG_OTHERS: &str = "others";
const EXPECTED_REVERT_VALUE: u16 = 9;
const SEED_VALUE: u64 = 821_577_831_833_715_345;
const TRANSFER_FROM_MAIN_PURSE_AMOUNT: u64 = 2_000_000_u64;

#[ignore]
#[test]
fn host_function_metrics_has_acceptable_size() {
    let size = utils::read_wasm_file_bytes(CONTRACT_HOST_FUNCTION_METRICS).len();
    assert!(
        size <= HOST_FUNCTION_METRICS_MAX_SIZE,
        "Performance regression: contract host-function-metrics became {} bytes long; up to {} bytes long would be acceptable.",
        size,
        HOST_FUNCTION_METRICS_MAX_SIZE
    );
    println!(
        "contract host-function-metrics byte size: {}, ubound: {}",
        size, HOST_FUNCTION_METRICS_MAX_SIZE
    )
}

fn create_account_exec_request(address: AccountHash) -> ExecuteRequest {
    ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT_U512,
        runtime_args! {
            ARG_TARGET => address,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        },
    )
    .build()
}

#[ignore]
#[test]
fn host_function_metrics_has_acceptable_gas_cost() {
    let mut builder = setup();

    let seed: u64 = SEED_VALUE;
    let random_bytes = {
        let mut random_bytes = vec![0_u8; 10_000];
        for (i, byte) in random_bytes.iter_mut().enumerate() {
            *byte = i.checked_rem(256).unwrap().try_into().unwrap();
        }
        random_bytes
    };

    let exec_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(ACCOUNT0_ADDR)
            .with_deploy_hash([55; 32])
            .with_session_code(
                CONTRACT_HOST_FUNCTION_METRICS,
                runtime_args! {
                    ARG_SEED => seed,
                    ARG_OTHERS => (Bytes::from(random_bytes), ACCOUNT0_ADDR, ACCOUNT1_ADDR),
                    ARG_AMOUNT => TRANSFER_FROM_MAIN_PURSE_AMOUNT,
                },
            )
            .with_empty_payment_bytes(
                runtime_args! { standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT },
            )
            .with_authorization_keys(&[ACCOUNT0_ADDR])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder.exec(exec_request);

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::User(user_error)))
            if user_error == EXPECTED_REVERT_VALUE
        ),
        "Expected revert but actual error is {:?}",
        error
    );

    let gas_cost = builder.last_exec_gas_cost().value();
    assert!(
        gas_cost <= U512::from(HOST_FUNCTION_METRICS_MAX_GAS_COST),
        "Performance regression: contract host-function-metrics used {} gas; it should use no more than {} gas.",
        gas_cost,
        HOST_FUNCTION_METRICS_MAX_GAS_COST
    );
}

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(create_account_exec_request(ACCOUNT0_ADDR))
        .expect_success()
        .commit()
        .exec(create_account_exec_request(ACCOUNT1_ADDR))
        .expect_success()
        .commit();
    builder
}
