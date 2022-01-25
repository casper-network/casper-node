use std::collections::VecDeque;

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
    storage::trie::Trie,
};
use casper_execution_engine::core::engine_state::engine_config::DEFAULT_MAX_DELEGATOR_SIZE_LIMIT;
use casper_types::{
    bytesrepr::{Bytes, ToBytes, U8_SERIALIZED_LENGTH},
    runtime_args,
    system::standard_payment::ARG_AMOUNT,
    EraId, ProtocolVersion, RuntimeArgs, U512,
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
const NAMED_KEYS_BLOAT_CONTRACT: &str = "named_keys_bloat.wasm";

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
    let (mut builder, protocol_version) = setup(Setup::Default);

    let exec_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash: [u8; 32] = [43; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(NAMED_KEYS_BLOAT_CONTRACT, session_args)
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
fn gh_2605_large_write_exact_trie_limit() {
    // Set up wasm config so it can address at most [`DEFAULT_MAX_STORED_VALUE_SIZE`] bytes (incl.
    // overhead). This way we can test the imposed StoredValue limits for contracts without
    // altering storage limits.]
    const BLOCK_TIME_MARKER: u64 = 123456789;

    let memory_limit = 146;
    assert!(memory_limit as usize * WASM_PAGE_SIZE > DEFAULT_MAX_STORED_VALUE_SIZE as usize);

    let (mut builder, protocol_version) = setup(Setup::MaxMemory(memory_limit));

    let exec_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash: [u8; 32] = [43; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code("trie_leaf_under_8mb.wasm", session_args)
            .with_empty_payment_bytes(runtime_args! {
                // Use full balance to cover huge gas cost of this operation
                ARG_AMOUNT => U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy)
            .with_protocol_version(protocol_version)
            .with_block_time(BLOCK_TIME_MARKER)
            .build()
    };

    builder.exec(exec_request).expect_success().commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let data_key = account.named_keys().get("data").expect("should have data");

    // Traverse the global state and extract all leaf tries
    let mut all_leafs = Vec::new();
    let mut stack = VecDeque::new();
    stack.push_back(builder.get_post_state_hash());

    while !stack.is_empty() {
        let pointer = stack.pop_front().unwrap();
        let trie = builder.get_trie(pointer).unwrap();
        match trie.clone() {
            Trie::Leaf { key, value: _value } => {
                all_leafs.push((key, trie));
            }
            Trie::Node { pointer_block } => {
                for (_i, pointer) in pointer_block.as_indexed_pointers() {
                    let hash = pointer.into_hash();
                    stack.push_back(hash);
                }
            }
            Trie::Extension {
                affix: _affix,
                pointer,
            } => {
                let hash = pointer.into_hash();
                stack.push_back(hash);
            }
        }
    }

    all_leafs.sort_by(|(_key1, a), (_key2, b)| b.serialized_length().cmp(&a.serialized_length()));

    // Biggest is the first one
    let (biggest_trie_key, biggest_trie_leaf) = &all_leafs[0];
    assert_eq!(
        biggest_trie_key.as_uref().unwrap().addr(),
        data_key.as_uref().unwrap().addr()
    );

    let (key, stored_value) = match biggest_trie_leaf {
        Trie::Leaf { key, value } => (key, value),
        _ => panic!("no"),
    };

    {
        let cl_value = stored_value.as_cl_value().cloned().unwrap();
        let bytes: Bytes = cl_value.into_t().unwrap();
        let vec: Vec<u8> = bytes.into();

        // Contract puts a block time as a marker at the beggining of CLValue bytes for reference
        let marker_bytes = BLOCK_TIME_MARKER.to_le_bytes();
        assert_eq!(&vec[0..8], &marker_bytes);
    }

    let computed_trie_size =
        U8_SERIALIZED_LENGTH + key.serialized_length() + stored_value.serialized_length();
    assert_eq!(
        biggest_trie_leaf.serialized_length(),
        DEFAULT_MAX_STORED_VALUE_SIZE as usize,
        "biggest serialized trie should not exceed the maximum limit"
    );
    assert_eq!(
        DEFAULT_MAX_STORED_VALUE_SIZE as usize, computed_trie_size,
        "should write excatly the limit"
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
        DEFAULT_MAX_DELEGATOR_SIZE_LIMIT,
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
