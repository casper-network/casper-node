use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::engine_state::ExecuteRequest;
use casper_types::{
    account::{AccountHash, ACCOUNT_HASH_LENGTH},
    bytesrepr::Bytes,
    runtime_args,
    system::standard_payment,
    ApiError, RuntimeArgs, U512,
};
use rand::prelude::{Rng, SeedableRng, StdRng};

const CONTRACT_HOST_FUNCTION_METRICS: &str = "host_function_metrics.wasm";
const CONTRACT_TRANSFER_TO_ACCOUNT_U512: &str = "transfer_to_account_u512.wasm";

const HOST_FUNCTION_METRICS_STANDARD_SIZE: usize = 90_620;
const HOST_FUNCTION_METRICS_STANDARD_GAS_COST: u64 = 141_111_502_050;

const HOST_FUNCTION_METRICS_MIN_SIZE: usize = HOST_FUNCTION_METRICS_STANDARD_SIZE * 95 / 100;
const HOST_FUNCTION_METRICS_MAX_SIZE: usize = HOST_FUNCTION_METRICS_STANDARD_SIZE * 105 / 100;
const HOST_FUNCTION_METRICS_MIN_GAS_COST: u64 = HOST_FUNCTION_METRICS_STANDARD_GAS_COST * 95 / 100;
const HOST_FUNCTION_METRICS_MAX_GAS_COST: u64 = HOST_FUNCTION_METRICS_STANDARD_GAS_COST * 105 / 100;

const ACCOUNT0_ADDR: AccountHash = AccountHash::new([42; ACCOUNT_HASH_LENGTH]);
const ACCOUNT1_ADDR: AccountHash = AccountHash::new([43; ACCOUNT_HASH_LENGTH]);

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
    assert!(
        size >= HOST_FUNCTION_METRICS_MIN_SIZE,
        "Performance improvement: contract host-function-metrics became only {} bytes long; please adjust this regression test.",
        size
    );
}

fn create_account_exec_request(address: AccountHash) -> ExecuteRequest {
    ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT_U512,
        runtime_args! {
            "target" => address,
            "amount" => *DEFAULT_PAYMENT,
        },
    )
    .build()
}

#[ignore]
#[test]
fn host_function_metrics_has_acceptable_gas_cost() {
    let rng: &mut StdRng = &mut SeedableRng::seed_from_u64(0);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST)
        .exec(create_account_exec_request(ACCOUNT0_ADDR))
        .expect_success()
        .commit()
        .exec(create_account_exec_request(ACCOUNT1_ADDR))
        .expect_success()
        .commit();

    let seed: u64 = rng.gen();
    let mut random_bytes = vec![0_u8; 10_000];
    rng.fill(random_bytes.as_mut_slice());

    let deploy = DeployItemBuilder::new()
        .with_address(ACCOUNT0_ADDR)
        .with_deploy_hash(rng.gen())
        .with_session_code(
            CONTRACT_HOST_FUNCTION_METRICS,
            runtime_args! {
                "seed" => seed,
                "others" => (Bytes::from(random_bytes), ACCOUNT0_ADDR, ACCOUNT1_ADDR),
            },
        )
        .with_empty_payment_bytes(
            runtime_args! { standard_payment::ARG_AMOUNT => U512::from(1_000_000_000_000_u64) },
        )
        .with_authorization_keys(&[ACCOUNT0_ADDR])
        .build();
    let exec_request = ExecuteRequestBuilder::new().push_deploy(deploy).build();

    builder.exec(exec_request);
    let error_message = builder
        .exec_error_message(2)
        .expect("should have an error message");
    assert!(error_message.contains(&format!("{:?}", ApiError::User(9))));

    let gas_cost = builder.last_exec_gas_cost().value();
    assert!(
        gas_cost <= U512::from(HOST_FUNCTION_METRICS_MAX_GAS_COST),
        "Performance regression: contract host-function-metrics used {} gas; it should use no more than {} gas.",
        gas_cost,
        HOST_FUNCTION_METRICS_MAX_GAS_COST
    );
    assert!(
        gas_cost >= U512::from(HOST_FUNCTION_METRICS_MIN_GAS_COST),
        "Performance improvement: contract host-function-metrics used only {} gas; please adjust this regression test.",
        gas_cost
    );
}
