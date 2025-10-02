use std::{env, path::PathBuf, sync::Arc};

use casper_executor_wasm::{
    chainspec_config::{self, ChainspecConfig, DEFAULT_ACCOUNT_HASH},
    install::InstallContractResult,
    testing::{
        base_execute_builder, base_install_request_builder, make_address_generator, make_executor,
        make_global_state_with_genesis, read_wasm, run_create_contract, run_wasm_session,
        TRANSACTION_HASH,
    },
    ExecutorV2,
};
use casper_executor_wasm_interface::executor::ExecutionKind;
use casper_storage::{
    data_access_layer::{QueryRequest, QueryResult},
    global_state::state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
    AddressGenerator,
};
use casper_types::{BlockHash, Digest, EntityAddr, Key, Timestamp};
use once_cell::sync::Lazy;
use parking_lot::{lock_api::RwLock, RawRwLock};
use tempfile::TempDir;

/// Symlink to chainspec.
pub static CHAINSPEC_SYMLINK: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/")
        .join(chainspec_config::CHAINSPEC_NAME)
});

#[test]
fn should_store_initial_state() {
    let (
        global_state,
        state_root_hash,
        create_result,
        _chainspec_config,
        _address_generator,
        _executor,
        _tmpdir,
    ) = install_hello_world();
    let contract_hash = EntityAddr::SmartContract(*create_result.smart_contract_addr());

    let post_state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let value = match global_state.query(QueryRequest::new(
        post_state_root_hash,
        Key::State(contract_hash),
        vec![],
    )) {
        QueryResult::Success { value, proofs: _ } => value,
        _ => unreachable!("Expected persisted state"),
    };
    let stringified: String = borsh::from_slice(
        &value
            .as_cl_value()
            .expect("expected cl value")
            .inner_bytes(),
    )
    .expect("expected inner state to be string");

    assert_eq!(stringified, "Hello world")
}

#[test]
fn should_store_state_after_changes() {
    let (
        global_state,
        state_root_hash,
        create_result,
        chainspec_config,
        address_generator,
        mut executor,
        _tmpdir,
    ) = install_hello_world();
    let contract_address = *create_result.smart_contract_addr();
    let contract_hash = EntityAddr::SmartContract(contract_address);

    let post_state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let run_method = base_execute_builder(&chainspec_config)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_address,
            entry_point: "spanish".to_owned(),
        })
        .build()
        .expect("should build");
    let execution_result = run_wasm_session(
        &mut executor,
        &global_state,
        post_state_root_hash,
        run_method,
    )
    .expect("Expected execution");
    let post_state_root_hash = global_state
        .commit_effects(post_state_root_hash, execution_result.effects().clone())
        .expect("Should commit");

    let run_method = base_execute_builder(&chainspec_config)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_address,
            entry_point: "french".to_owned(),
        })
        .build()
        .expect("should build");
    let execution_result = run_wasm_session(
        &mut executor,
        &global_state,
        post_state_root_hash,
        run_method,
    )
    .expect("Expected execution");
    let post_state_root_hash = global_state
        .commit_effects(post_state_root_hash, execution_result.effects().clone())
        .expect("Should commit");

    let value = match global_state.query(QueryRequest::new(
        post_state_root_hash,
        Key::State(contract_hash),
        vec![],
    )) {
        QueryResult::Success { value, proofs: _ } => value,
        _ => unreachable!("Expected persisted state"),
    };
    let stringified: String = borsh::from_slice(
        &value
            .as_cl_value()
            .expect("expected cl value")
            .inner_bytes(),
    )
    .expect("expected inner state to be string");

    assert_eq!(stringified, "Bonjour le monde")
}

#[test]
fn should_fetch_data_with_contract_method() {
    let (
        global_state,
        state_root_hash,
        create_result,
        chainspec_config,
        address_generator,
        mut executor,
        _tmpdir,
    ) = install_hello_world();
    let contract_address = *create_result.smart_contract_addr();

    let post_state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let run_method = base_execute_builder(&chainspec_config)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_address,
            entry_point: "spanish".to_owned(),
        })
        .build()
        .expect("should build");
    let execution_result = run_wasm_session(
        &mut executor,
        &global_state,
        post_state_root_hash,
        run_method,
    )
    .expect("Expected execution");
    let post_state_root_hash = global_state
        .commit_effects(post_state_root_hash, execution_result.effects().clone())
        .expect("Should commit");

    let run_method = base_execute_builder(&chainspec_config)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_address,
            entry_point: "get".to_owned(),
        })
        .build()
        .expect("should build");
    let execution_result = run_wasm_session(
        &mut executor,
        &global_state,
        post_state_root_hash,
        run_method,
    )
    .expect("Expected execution");
    let raw_output = execution_result.output().expect("expected_value");
    let stringified: String = borsh::from_slice(&raw_output).expect("Expected successfull");
    assert_eq!(stringified, "Hola Mundo");
}

fn install_hello_world() -> (
    LmdbGlobalState,
    Digest,
    InstallContractResult,
    ChainspecConfig,
    Arc<RwLock<RawRwLock, AddressGenerator>>,
    ExecutorV2,
    TempDir,
) {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (global_state, state_root_hash, tempdir) = make_global_state_with_genesis();
    let block_time_1 = Timestamp::now().into();
    let address_generator = make_address_generator();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(read_wasm("vm2_hello_world.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_block_time(block_time_1)
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(1) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );
    (
        global_state,
        state_root_hash,
        create_result,
        chainspec_config,
        address_generator,
        executor,
        tempdir,
    )
}
