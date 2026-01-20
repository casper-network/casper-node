use std::{env, path::PathBuf, sync::Arc};

use casper_execution_engine::engine_state::{EngineConfig, ExecutionEngineV1};
use casper_executor_wasm::{
    testing::{
        base_execute_builder, base_install_request_builder, make_address_generator,
        make_global_state_with_genesis, read_wasm, run_create_contract, run_wasm_session,
    },
    ExecutorConfigBuilder, ExecutorKind, ExecutorV2,
};

use casper_executor_wasm::{chainspec_config, chainspec_config::ChainspecConfig};
use casper_executor_wasm_interface::executor::{ExecutionKind, PackagePointer};
use casper_storage::global_state::state::CommitProvider;
use once_cell::sync::Lazy;

/// Symlink to chainspec.
pub static CHAINSPEC_SYMLINK: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/")
        .join(chainspec_config::CHAINSPEC_NAME)
});

#[test]
fn should_run_test_suite() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");
    let storage_costs = chainspec_config.storage_costs;
    let mint_costs = chainspec_config.system_costs_config.mint_costs().clone();
    let auction_costs = chainspec_config.system_costs_config.auction_costs().clone();
    let v1_config = EngineConfig::from(chainspec_config.clone());
    let execution_engine_v1 = ExecutionEngineV1::new(v1_config);
    let wasm_v2_config = *chainspec_config.wasm_config.v2();
    let memory_limit = wasm_v2_config.max_memory();
    let message_limits = chainspec_config.wasm_config.messages_limits();
    let executor_config = ExecutorConfigBuilder::default()
        .with_memory_limit(memory_limit)
        .with_executor_kind(ExecutorKind::Compiled)
        .with_wasm_config(wasm_v2_config)
        .with_storage_costs(storage_costs)
        .with_mint_costs(mint_costs)
        .with_auction_costs(auction_costs)
        .with_baseline_motes_amount(chainspec_config.core_config.baseline_motes_amount)
        .with_message_limits(message_limits)
        .build()
        .expect("Should build");
    let mut executor = ExecutorV2::new(executor_config, execution_engine_v1);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let vm2_collections_test = read_wasm("vm2_collections_test.wasm");

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(vm2_collections_test.wasm)
        .with_bundle_data(vm2_collections_test.meta.expect("should have bundle data"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );

    let contract_address = *create_result.smart_contract_addr();
    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let run_method = base_execute_builder(&chainspec_config)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_execution_kind(ExecutionKind::Stored {
            package_pointer: PackagePointer::HashAddr(contract_address),
            entry_point: "assertions".to_owned(),
            version: None,
            protocol_version_major: None,
        })
        .build()
        .expect("should build");
    let res = run_wasm_session(&mut executor, &global_state, state_root_hash, run_method);
    assert!(res.is_ok());
    if let Ok(res) = res {
        assert!(res.host_error.is_none());
    }
}

#[test]
fn inserting_into_non_existing_vec_index_fails() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");
    let storage_costs = chainspec_config.storage_costs;
    let mint_costs = chainspec_config.system_costs_config.mint_costs().clone();
    let auction_costs = chainspec_config.system_costs_config.auction_costs().clone();
    let v1_config = EngineConfig::from(chainspec_config.clone());
    let execution_engine_v1 = ExecutionEngineV1::new(v1_config);
    let wasm_v2_config = *chainspec_config.wasm_config.v2();
    let memory_limit = wasm_v2_config.max_memory();
    let message_limits = chainspec_config.wasm_config.messages_limits();
    let executor_config = ExecutorConfigBuilder::default()
        .with_memory_limit(memory_limit)
        .with_executor_kind(ExecutorKind::Compiled)
        .with_wasm_config(wasm_v2_config)
        .with_storage_costs(storage_costs)
        .with_mint_costs(mint_costs)
        .with_auction_costs(auction_costs)
        .with_baseline_motes_amount(chainspec_config.core_config.baseline_motes_amount)
        .with_message_limits(message_limits)
        .build()
        .expect("Should build");
    let mut executor = ExecutorV2::new(executor_config, execution_engine_v1);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let vm2_collections_test = read_wasm("vm2_collections_test.wasm");

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(vm2_collections_test.wasm)
        .with_bundle_data(vm2_collections_test.meta.expect("should have bundle data"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );

    let contract_address = *create_result.smart_contract_addr();
    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let run_method = base_execute_builder(&chainspec_config)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_execution_kind(ExecutionKind::Stored {
            package_pointer: PackagePointer::HashAddr(contract_address),
            entry_point: "test_remove_invalid_index_prepare".to_owned(),
            version: None,
            protocol_version_major: None,
        })
        .build()
        .expect("should build");
    let res = run_wasm_session(&mut executor, &global_state, state_root_hash, run_method);
    assert!(res.is_ok());

    if let Ok(res) = res {
        assert!(matches!(
            res.host_error,
            Some(casper_executor_wasm_common::error::CallError::NotCallable)
        ));
    }
}
