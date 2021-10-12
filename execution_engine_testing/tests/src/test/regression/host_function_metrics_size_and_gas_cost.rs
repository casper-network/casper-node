use casper_engine_test_support::{
    internal::{
        utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_PAYMENT,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
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

const MAX_SIZE_HOST_FUNCTION_METRICS: usize = 95511;
const MAX_GAS_COST_HOST_FUNCTION_METRICS: u64 = 148_167_077_151;

const ACCOUNT0_ADDR: AccountHash = AccountHash::new([42; ACCOUNT_HASH_LENGTH]);
const ACCOUNT1_ADDR: AccountHash = AccountHash::new([43; ACCOUNT_HASH_LENGTH]);

#[test]
fn host_function_metrics_is_small() {
    let size = utils::read_wasm_file_bytes(CONTRACT_HOST_FUNCTION_METRICS).len();
    assert!(size <= MAX_SIZE_HOST_FUNCTION_METRICS,
            "Contract host_function_metrics must be at most {} bytes long, but is {} bytes long instead.",
            MAX_SIZE_HOST_FUNCTION_METRICS, size);
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

#[test]
fn host_function_metrics_costs_little_gas() {
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

    let gas_cost = builder.last_exec_gas_cost();
    assert!(
        gas_cost.value() <= U512::from(MAX_GAS_COST_HOST_FUNCTION_METRICS),
        "Running host_function_metrics cost {} gas, but shouldn't have cost more than {} gas.",
        gas_cost,
        MAX_GAS_COST_HOST_FUNCTION_METRICS
    );
}
