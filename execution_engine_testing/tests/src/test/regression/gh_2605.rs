use once_cell::sync::Lazy;
use regex::Regex;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_MAX_ASSOCIATED_KEYS,
    DEFAULT_MAX_STORED_VALUE_SIZE, DEFAULT_PROTOCOL_VERSION, DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    core::{
        engine_state::{
            EngineConfig, Error, UpgradeConfig, DEFAULT_MAX_QUERY_DEPTH,
            DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
        },
        execution,
    },
    shared::{
        host_function_costs::HostFunctionCosts,
        opcode_costs::OpcodeCosts,
        storage_costs::StorageCosts,
        system_config::SystemConfig,
        wasm_config::{WasmConfig, DEFAULT_MAX_STACK_HEIGHT},
    },
};
use casper_types::{
    runtime_args, system::standard_payment::ARG_AMOUNT, EraId, ProtocolVersion, RuntimeArgs, U512,
};

// Linear memory under Wasm VM is a multiply of 64 Ki
// https://webassembly.github.io/spec/core/exec/runtime.html#page-size
const WASM_PAGE_SIZE: usize = 65_536;

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});
const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);
const BIG_TRIE_LEAF_CONTRACT: &str = "big_trie_leaf.wasm";

enum Setup {
    Default,
    MaxMemory(u32),
}

static WASM_MEMORY_ERROR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^maximum limit \d+ is less than minimum \d+$").unwrap());

#[ignore]
#[test]
fn gh_2605_large_contract_write_should_fail_with_default_config() {
    let (mut builder, protocol_version) = setup(Setup::Default);

    let exec_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash: [u8; 32] = [43; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(BIG_TRIE_LEAF_CONTRACT, session_args)
            .with_empty_payment_bytes(runtime_args! {
                // Use full balance to cover huge gas cost of this operation
                ARG_AMOUNT => U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy)
            .with_protocol_version(protocol_version)
            .build()
    };

    builder.exec(exec_request);

    let error = &builder.get_error().expect("should error");

    assert!(
        matches!(error, Error::Exec(execution::Error::Interpreter(msg)) if WASM_MEMORY_ERROR_RE.is_match(msg)),
        "expected wasm memory error but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn gh_2605_large_contract_write_should_fail() {
    // Set up wasm config so it can address at most [`DEFAULT_MAX_STORED_VALUE_SIZE`] bytes (incl.
    // overhead). This way we can test the imposed StoredValue limits for contracts without
    // altering storage limits.
    let memory_limit = 146;
    assert!(memory_limit as usize * WASM_PAGE_SIZE > DEFAULT_MAX_STORED_VALUE_SIZE as usize);

    let (mut builder, protocol_version) = setup(Setup::MaxMemory(memory_limit));

    let exec_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash: [u8; 32] = [43; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(BIG_TRIE_LEAF_CONTRACT, session_args)
            .with_empty_payment_bytes(runtime_args! {
                // Use full balance to cover huge gas cost of this operation
                ARG_AMOUNT => U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy)
            .with_protocol_version(protocol_version)
            .build()
    };

    builder.exec(exec_request);

    let error = builder.get_error().expect("should error");
    assert!(
        matches!(error, Error::Exec(execution::Error::ValueTooLarge)),
        "expected ValueTooLarge but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn gh_2605_should_not_allow_to_exceed_8mb_named_keys() {
    // Set up wasm config so it can address at most [`DEFAULT_MAX_STORED_VALUE_SIZE`] bytes (incl.
    // overhead). This way we can test the imposed StoredValue limits for contracts without
    // altering storage limits.
    let memory_limit = 146;
    assert!(memory_limit as usize * WASM_PAGE_SIZE > DEFAULT_MAX_STORED_VALUE_SIZE as usize);

    let (mut builder, protocol_version) = setup(Setup::MaxMemory(memory_limit));

    let exec_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash: [u8; 32] = [43; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code("named_keys_bloat.wasm", session_args)
            .with_empty_payment_bytes(runtime_args! {
                // Use full balance to cover huge gas cost of this operation
                ARG_AMOUNT => U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy)
            .with_protocol_version(protocol_version)
            .build()
    };

    builder.exec(exec_request);

    let error = builder.get_error().expect("should error");
    assert!(
        matches!(error, Error::Exec(execution::Error::ValueTooLarge)),
        "expected ValueTooLarge but received {:?}",
        error
    );
}

fn setup(custom_upgrade_request: Setup) -> (InMemoryWasmTestBuilder, ProtocolVersion) {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let mut upgrade_config = make_upgrade_request(*OLD_PROTOCOL_VERSION, *NEW_PROTOCOL_VERSION);
    let engine_config = match custom_upgrade_request {
        Setup::Default => EngineConfig::default(),
        Setup::MaxMemory(max_memory) => make_engine_config(max_memory),
    };
    builder.upgrade_with_upgrade_request(engine_config, &mut upgrade_config);

    (builder, *NEW_PROTOCOL_VERSION)
}

fn make_engine_config(max_memory: u32) -> EngineConfig {
    let wasm_config = make_wasm_config(max_memory);
    EngineConfig::new(
        DEFAULT_MAX_QUERY_DEPTH,
        DEFAULT_MAX_ASSOCIATED_KEYS,
        DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
        DEFAULT_MAX_STORED_VALUE_SIZE,
        wasm_config,
        SystemConfig::default(),
    )
}

fn make_wasm_config(max_memory: u32) -> WasmConfig {
    WasmConfig::new(
        max_memory,
        DEFAULT_MAX_STACK_HEIGHT,
        OpcodeCosts::default(),
        StorageCosts::default(),
        HostFunctionCosts::default(),
    )
}

fn make_upgrade_request(
    old_version: ProtocolVersion,
    new_version: ProtocolVersion,
) -> UpgradeConfig {
    UpgradeRequestBuilder::new()
        .with_current_protocol_version(old_version)
        .with_new_protocol_version(new_version)
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .build()
}
