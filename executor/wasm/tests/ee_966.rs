use std::sync::Arc;

use bytes::Bytes;
use casper_execution_engine::engine_state::ExecutionEngineV1;
use casper_executor_wasm::{ExecutorConfigBuilder, ExecutorKind, ExecutorV2};
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::{
    executor::{
        ExecuteError, ExecuteRequestBuilder, ExecuteWithProviderError, ExecuteWithProviderResult,
        ExecutionKind,
    },
    WasmPreparationError,
};
use casper_storage::RuntimeNativeConfig;
use casper_types::{
    BlockHash, Chainspec, Digest, Key, MessageLimits, StorageCosts, Timestamp, WasmV2Config,
    DEFAULT_WASM_MAX_MEMORY,
};
use casper_wasm::builder;

use casper_executor_wasm::testing::{
    expect_successful_execution, make_address_generator, make_executor,
    make_global_state_with_genesis, read_wasm, run_wasm_session, DEFAULT_ACCOUNT_HASH,
    DEFAULT_CHAIN_NAME, DEFAULT_GAS_LIMIT, DEFAULT_GAS_PER_BYTE_COST, TRANSACTION_HASH,
};

pub const CONTRACT_EE_966_REGRESSION: &str = "vm2_ee_966_regression.wasm";

fn make_session_code_with_memory_pages(initial_pages: u32, max_pages: Option<u32>) -> Bytes {
    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
        .build()
        .build()
        // Export above function
        .export()
        .field("call")
        .build()
        // Memory section is mandatory
        .memory()
        // Produces entry `(memory (0) initial_pages [max_pages])`
        .with_min(initial_pages)
        .with_max(max_pages)
        .build()
        .build();
    casper_wasm::serialize(module)
        .expect("should serialize")
        .into()
}

#[test]
fn argument_size_exceeds_memory_limit() {
    use casper_executor_wasm_interface::executor::ExecuteError;
    let executor = {
        let storage_costs = StorageCosts::new(DEFAULT_GAS_PER_BYTE_COST);
        let execution_engine_v1 = ExecutionEngineV1::default();
        // Config with one page of mem (64KiB)
        let executor_config = ExecutorConfigBuilder::default()
            .with_memory_limit(1)
            .with_executor_kind(ExecutorKind::Compiled)
            .with_wasm_config(WasmV2Config::default())
            .with_storage_costs(storage_costs)
            .with_message_limits(MessageLimits::default())
            .build()
            .expect("Should build");
        ExecutorV2::new(executor_config, Arc::new(execution_engine_v1))
    };
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();
    let chainspec = Chainspec::default();
    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&chainspec);
    // Create an input larger than 1 page
    let large_input = Bytes::from(vec![0u8; 70_000]);
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(read_wasm("vm2_cep18.wasm")))
        .with_input(large_input)
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");
    let result = executor.execute_with_provider(state_root_hash, &global_state, execute_request);
    match result {
        Err(ExecuteWithProviderError::Execute(ExecuteError::ArgumentSizeExceedsMemory {
            argument_size,
            memory_limit,
        })) => {
            assert!(argument_size > (memory_limit as usize * 65536));
        }
        other => panic!("Expected ArgumentSizeExceedsMemory error, got: {:?}", other),
    }
}

#[test]
fn should_run_ee_966_with_zero_min_and_zero_max_memory() {
    // A contract that has initial memory pages of 0 and maximum memory pages of 0 is valid
    let session_code = make_session_code_with_memory_pages(0, Some(0));

    let mut executor = make_executor();
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&Chainspec::default());
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(session_code))
        .with_input(Bytes::new())
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");

    expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );
}

#[test]
fn should_run_ee_966_cant_have_too_much_initial_memory() {
    // Set initial memory to max + 1
    let session_code = make_session_code_with_memory_pages(DEFAULT_WASM_MAX_MEMORY + 1, None);

    let mut executor = make_executor();
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&Chainspec::default());
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(session_code))
        .with_input(Bytes::new())
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");

    let result = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    assert!(matches!(
        result,
        Err(ExecuteWithProviderError::Execute(
            ExecuteError::WasmPreparation(WasmPreparationError::Memory(_))
        ))
    ));
}

#[test]
fn should_run_ee_966_cant_have_too_much_max_memory() {
    let session_code = make_session_code_with_memory_pages(0, Some(DEFAULT_WASM_MAX_MEMORY + 1));

    let mut executor = make_executor();
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&Chainspec::default());
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(session_code))
        .with_input(Bytes::new())
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");

    let result = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    assert!(matches!(
        result,
        Err(ExecuteWithProviderError::Execute(
            ExecuteError::WasmPreparation(WasmPreparationError::Instantiation(_))
        ))
    ));
}

#[test]
fn should_run_ee_966_cant_have_way_too_much_max_memory() {
    let session_code = make_session_code_with_memory_pages(0, Some(DEFAULT_WASM_MAX_MEMORY * 3));

    let mut executor = make_executor();
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&Chainspec::default());
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(session_code))
        .with_input(Bytes::new())
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");

    let result = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    assert!(matches!(
        result,
        Err(ExecuteWithProviderError::Execute(
            ExecuteError::WasmPreparation(WasmPreparationError::Instantiation(_))
        ))
    ));
}

#[test]
fn should_run_ee_966_cant_have_larger_initial_than_max_memory() {
    let session_code = make_session_code_with_memory_pages(DEFAULT_WASM_MAX_MEMORY, Some(0));

    let mut executor = make_executor();
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&Chainspec::default());
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(session_code))
        .with_input(Bytes::new())
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");

    let result = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    assert!(matches!(
        result,
        Err(ExecuteWithProviderError::Execute(
            ExecuteError::WasmPreparation(WasmPreparationError::Compile(_))
        ))
    ));
}

#[test]
fn should_run_ee_966_should_request_exactly_maximum_as_initial() {
    let session_code = make_session_code_with_memory_pages(DEFAULT_WASM_MAX_MEMORY, None);

    let mut executor = make_executor();
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&Chainspec::default());
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(session_code))
        .with_input(Bytes::new())
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");

    let result = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    assert!(result.is_ok());
}

#[test]
fn should_run_ee_966_should_request_exactly_maximum() {
    let session_code =
        make_session_code_with_memory_pages(DEFAULT_WASM_MAX_MEMORY, Some(DEFAULT_WASM_MAX_MEMORY));

    let mut executor = make_executor();
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&Chainspec::default());
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(session_code))
        .with_input(Bytes::new())
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");

    let result = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    assert!(result.is_ok());
}

#[test]
fn should_run_ee_966_regression_fail_when_growing_mem_past_max() {
    let session_code = read_wasm(CONTRACT_EE_966_REGRESSION);

    let mut executor = make_executor();
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let runtime_native_config = RuntimeNativeConfig::from_chainspec(&Chainspec::default());
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(session_code))
        .with_input(Bytes::new())
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
        .with_runtime_native_config(runtime_native_config)
        .build()
        .expect("should build");

    let result = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    println!("{:?}", result);

    assert!(matches!(
        result,
        Ok(ExecuteWithProviderResult {
            host_error: Some(CallError::CalleeReverted),
            ..
        })
    ));
}
