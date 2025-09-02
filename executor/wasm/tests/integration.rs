use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use casper_executor_wasm::{
    install::{InstallContractError, InstallContractRequest},
    testing::{
        base_execute_builder, base_install_request_builder, call_dummy_host_fn_by_name,
        expect_successful_execution, make_address_generator, make_executor,
        make_global_state_with_genesis, make_runtime_config, read_wasm, run_create_contract,
        run_wasm_session, DEFAULT_GAS_LIMIT, TRANSACTION_HASH,
    },
};

use casper_executor_wasm::{
    chainspec_config,
    chainspec_config::{ChainspecConfig, DEFAULT_ACCOUNT_HASH},
};
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::executor::{
    ExecuteError, ExecuteWithProviderError, ExecutionKind,
};
use casper_storage::{
    data_access_layer::{
        prefixed_values::{PrefixedValuesRequest, PrefixedValuesResult},
        MessageTopicsRequest, MessageTopicsResult, QueryRequest, QueryResult,
    },
    global_state::{
        state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
        transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
    KeyPrefix,
};
use casper_types::{
    account::AccountHash, bytesrepr::ToBytes, execution::RetValue, BlockHash, Digest, EntityAddr,
    Key, RuntimeArgs, StoredValue, Timestamp,
};
use fs_extra::dir;
use itertools::Itertools;
use once_cell::sync::Lazy;

/// Symlink to chainspec.
pub static CHAINSPEC_SYMLINK: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/")
        .join(chainspec_config::CHAINSPEC_NAME)
});

#[test]
fn harness() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let flipper_address;

    state_root_hash = {
        let input_data = borsh::to_vec(&("Foo Token".to_string(),))
            .map(Bytes::from)
            .unwrap();

        let install_request = base_install_request_builder(&chainspec_config)
            .with_wasm_bytes(read_wasm("vm2_cep18.wasm"))
            .with_shared_address_generator(Arc::clone(&address_generator))
            .with_transferred_value(0)
            .with_entry_point("new".to_string())
            .with_input(input_data)
            .build()
            .expect("should build");

        let create_result = run_create_contract(
            &mut executor,
            &global_state,
            state_root_hash,
            install_request,
        );

        flipper_address = *create_result.smart_contract_addr();

        global_state
            .commit_effects(state_root_hash, create_result.effects().clone())
            .expect("Should commit")
    };

    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(read_wasm("vm2-harness.wasm")))
        .with_serialized_input((flipper_address,))
        .expect("expected serialized input to be correct")
        .with_shared_address_generator(address_generator)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"bl0ck")))
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
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
fn cep18() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let input_data = borsh::to_vec(&("Foo Token".to_string(),))
        .map(Bytes::from)
        .unwrap();

    let block_time_1 = Timestamp::now().into();

    let create_request = base_install_request_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(read_wasm("vm2_cep18.wasm").clone())
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data)
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
        create_request,
    );

    let contract_hash = EntityAddr::SmartContract(*create_result.smart_contract_addr());

    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let msgs = global_state.prefixed_values(PrefixedValuesRequest::new(
        state_root_hash,
        KeyPrefix::MessageEntriesByEntity(contract_hash),
    ));
    let PrefixedValuesResult::Success {
        key_prefix: _,
        values,
    } = msgs
    else {
        panic!("Expected success")
    };

    {
        let mut topics_1 = values
            .iter()
            .filter_map(|stored_value| stored_value.as_message_topic_summary())
            .collect_vec();
        topics_1
            .sort_by_key(|topic| (topic.topic_name(), topic.blocktime(), topic.message_count()));

        assert_eq!(topics_1[0].topic_name(), "Transfer");
        assert_eq!(topics_1[0].message_count(), 1);
        assert_eq!(topics_1[0].blocktime(), block_time_1);
    }

    let block_time_2 = (block_time_1.value() + 1).into();
    assert_ne!(block_time_1, block_time_2);

    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(read_wasm(
            "vm2_cep18_caller.wasm",
        )))
        .with_serialized_input((create_result.smart_contract_addr(),))
        .expect("expected serialized input to be correct")
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(block_time_2)
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(2) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .build()
        .expect("should build");

    let result_2 = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    state_root_hash = global_state
        .commit_effects(state_root_hash, result_2.effects().clone())
        .expect("Should commit");

    let MessageTopicsResult::Success { message_topics } =
        global_state.message_topics(MessageTopicsRequest::new(state_root_hash, contract_hash))
    else {
        panic!("Expected success")
    };

    assert!(message_topics.get("Transfer").is_some());
    assert_ne!(
        message_topics.get("Mint"),
        message_topics.get("Transfer"),
        "Mint and Transfer topics should have different hashes"
    );

    {
        let msgs = global_state.prefixed_values(PrefixedValuesRequest::new(
            state_root_hash,
            KeyPrefix::MessageEntriesByEntity(contract_hash),
        ));
        let PrefixedValuesResult::Success {
            key_prefix: _,
            values,
        } = msgs
        else {
            panic!("Expected success")
        };

        let mut topics_2 = values
            .iter()
            .filter_map(|stored_value| stored_value.as_message_topic_summary())
            .collect_vec();
        topics_2
            .sort_by_key(|topic| (topic.topic_name(), topic.blocktime(), topic.message_count()));

        assert_eq!(topics_2.len(), 1);
        assert_eq!(topics_2[0].topic_name(), "Transfer");
        assert_eq!(topics_2[0].message_count(), 2);
        assert_eq!(topics_2[0].blocktime(), block_time_2); // NOTE: Session called mint; the topic
                                                           // summary blocktime is refreshed
    }

    let mut messages = result_2.messages().iter().collect_vec();
    messages.sort_by_key(|message| {
        (
            message.topic_name(),
            message.topic_index(),
            message.block_index(),
        )
    });
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].topic_name(), "Transfer");
    assert_eq!(messages[0].topic_index(), 0);
    assert_eq!(messages[0].block_index(), 0);

    assert_eq!(messages[1].topic_name(), "Transfer");
    assert_eq!(messages[1].topic_index(), 1);
    assert_eq!(messages[1].block_index(), 1);
}

#[test]
fn traits() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let execute_request = base_execute_builder(&chainspec_config)
        .with_target(ExecutionKind::SessionBytes(read_wasm("vm2_trait.wasm")))
        .with_serialized_input(())
        .expect("expected serialized input to be correct")
        .with_shared_address_generator(make_address_generator())
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
fn assoc_keys() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let execute_request = base_execute_builder(&chainspec_config)
        .with_target(ExecutionKind::SessionBytes(read_wasm(
            "vm2_assoc_keys.wasm",
        )))
        .with_serialized_input(3u8)
        .expect("expected serialized input to be correct")
        .with_shared_address_generator(make_address_generator())
        .build()
        .expect("should build");

    let add_assoc_keys_result = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    let state_root_hash = global_state
        .commit_effects(state_root_hash, add_assoc_keys_result.effects().clone())
        .expect("Should commit");

    let query_request = QueryRequest::new(
        state_root_hash,
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_HASH.value())),
        vec![],
    );

    let query_result = global_state.query(query_request);
    if let QueryResult::Success { value, .. } = query_result {
        let entity = value.as_addressable_entity().expect("must get entity");
        assert!(entity.associated_keys().len() == 2)
    } else {
        panic!("Unexpected query result")
    }

    let execute_request = base_execute_builder(&chainspec_config)
        .with_target(ExecutionKind::SessionBytes(read_wasm(
            "vm2_assoc_keys.wasm",
        )))
        .with_serialized_input(0u8)
        .expect("expected serialized input to be correct")
        .with_shared_address_generator(make_address_generator())
        .build()
        .expect("should build");

    let remove_assoc_key = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    let state_root_hash = global_state
        .commit_effects(state_root_hash, remove_assoc_key.effects().clone())
        .expect("Should commit");

    let query_request = QueryRequest::new(
        state_root_hash,
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_HASH.value())),
        vec![],
    );

    let query_result = global_state.query(query_request);
    if let QueryResult::Success { value, .. } = query_result {
        let entity = value.as_addressable_entity().expect("must get entity");
        assert!(entity.associated_keys().len() == 1)
    } else {
        panic!("Unexpected query result")
    }
}

#[test]
fn upgradable() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let upgradable_address;

    state_root_hash = {
        let input_data = borsh::to_vec(&(0u8,)).map(Bytes::from).unwrap();

        let create_request = base_install_request_builder(&chainspec_config)
            .with_wasm_bytes(read_wasm("vm2_upgradable.wasm"))
            .with_shared_address_generator(Arc::clone(&address_generator))
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_entry_point("new".to_string())
            .with_input(input_data)
            .build()
            .expect("should build");

        let create_result = run_create_contract(
            &mut executor,
            &global_state,
            state_root_hash,
            create_request,
        );

        upgradable_address = *create_result.smart_contract_addr();

        global_state
            .commit_effects(state_root_hash, create_result.effects().clone())
            .expect("Should commit")
    };

    let version_before_upgrade = {
        let execute_request = base_execute_builder(&chainspec_config)
            .with_target(ExecutionKind::Stored {
                address: upgradable_address,
                entry_point: "version".to_string(),
            })
            .with_input(Bytes::new())
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_shared_address_generator(Arc::clone(&address_generator))
            .build()
            .expect("should build");
        let res = expect_successful_execution(
            &mut executor,
            &global_state,
            state_root_hash,
            execute_request,
        );
        let output = res.output().expect("should have output");
        let version: String = borsh::from_slice(output).expect("should deserialize");
        version
    };
    assert_eq!(version_before_upgrade, "v1");

    {
        // Increment the value
        let execute_request = base_execute_builder(&chainspec_config)
            .with_target(ExecutionKind::Stored {
                address: upgradable_address,
                entry_point: "increment".to_string(),
            })
            .with_input(Bytes::new())
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_shared_address_generator(Arc::clone(&address_generator))
            .build()
            .expect("should build");
        let res = expect_successful_execution(
            &mut executor,
            &global_state,
            state_root_hash,
            execute_request,
        );
        state_root_hash = global_state
            .commit_effects(state_root_hash, res.effects().clone())
            .expect("Should commit");
    };

    let binding = read_wasm("vm2_upgradable_v2.wasm");
    let new_code = binding.as_ref();

    let execute_request = base_execute_builder(&chainspec_config)
        .with_transferred_value(0)
        .with_target(ExecutionKind::Stored {
            address: upgradable_address,
            entry_point: "perform_upgrade".to_string(),
        })
        .with_gas_limit(DEFAULT_GAS_LIMIT * 10)
        .with_serialized_input((new_code,))
        .expect("expected serialized input to be correct")
        .with_shared_address_generator(Arc::clone(&address_generator))
        .build()
        .expect("should build");
    let res = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );
    state_root_hash = global_state
        .commit_effects(state_root_hash, res.effects().clone())
        .expect("Should commit");

    let version_after_upgrade = {
        let execute_request = base_execute_builder(&chainspec_config)
            .with_target(ExecutionKind::Stored {
                address: upgradable_address,
                entry_point: "version".to_string(),
            })
            .with_input(Bytes::new())
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_shared_address_generator(Arc::clone(&address_generator))
            .build()
            .expect("should build");
        let res = expect_successful_execution(
            &mut executor,
            &global_state,
            state_root_hash,
            execute_request,
        );
        let output = res.output().expect("should have output");
        let version: String = borsh::from_slice(output).expect("should deserialize");
        version
    };
    assert_eq!(version_after_upgrade, "v2");

    {
        // Increment the value
        let execute_request = base_execute_builder(&chainspec_config)
            .with_target(ExecutionKind::Stored {
                address: upgradable_address,
                entry_point: "increment_by".to_string(),
            })
            .with_serialized_input((10u64,))
            .expect("expected serialized input to be correct")
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_shared_address_generator(Arc::clone(&address_generator))
            .build()
            .expect("should build");
        let res = expect_successful_execution(
            &mut executor,
            &global_state,
            state_root_hash,
            execute_request,
        );
        state_root_hash = global_state
            .commit_effects(state_root_hash, res.effects().clone())
            .expect("Should commit");
    };

    let _ = state_root_hash;
}

#[test]
fn backwards_compatibility() {
    let (global_state, post_state_hash, _temp) = {
        let fixture_name = "counter_contract";
        // /Users/michal/Dev/casper-node/execution_engine_testing/tests/fixtures/counter_contract/
        // global_state/data.lmdb
        let lmdb_fixtures_base_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../")
            .join("../")
            .join("execution_engine_testing")
            .join("tests")
            .join("fixtures");
        assert!(lmdb_fixtures_base_dir.exists());

        let source = lmdb_fixtures_base_dir.join("counter_contract");
        let to = tempfile::tempdir().expect("should create temp dir");
        fs_extra::copy_items(&[source], &to, &dir::CopyOptions::default())
            .expect("should copy global state fixture");

        let path_to_state = to.path().join(fixture_name).join("state.json");

        let lmdb_fixture_state: serde_json::Value =
            serde_json::from_reader(File::open(path_to_state).unwrap()).unwrap();
        let post_state_hash =
            Digest::from_hex(lmdb_fixture_state["post_state_hash"].as_str().unwrap()).unwrap();

        let path_to_gs = to.path().join(fixture_name).join("global_state");

        const DEFAULT_LMDB_PAGES: usize = 256_000_000;
        const DEFAULT_MAX_READERS: u32 = 512;

        let environment = LmdbEnvironment::new(
            &path_to_gs,
            16384 * DEFAULT_LMDB_PAGES,
            DEFAULT_MAX_READERS,
            true,
        )
        .expect("should create LmdbEnvironment");

        let trie_store =
            LmdbTrieStore::open(&environment, None).expect("should open LmdbTrieStore");
        (
            LmdbGlobalState::new(
                Arc::new(environment),
                Arc::new(trie_store),
                post_state_hash,
                100,
                false,
            ),
            post_state_hash,
            to,
        )
    };

    let result = global_state.query(QueryRequest::new(
        post_state_hash,
        Key::Account(*DEFAULT_ACCOUNT_HASH),
        Vec::new(),
    ));
    let value = match result {
        QueryResult::RootNotFound => todo!(),
        QueryResult::ValueNotFound(value) => panic!("Value not found: {:?}", value),
        QueryResult::Success { value, .. } => value,
        QueryResult::Failure(failure) => panic!("Failed to query: {:?}", failure),
    };

    //
    // Calling VM1 contract directly by its address
    //

    let mut state_root_hash = post_state_hash;

    let value = match *value {
        StoredValue::Account(account) => account,
        _ => panic!("Expected CLValue"),
    };

    let counter_hash = match value.named_keys().get("counter") {
        Some(Key::Hash(hash_address)) => hash_address,
        _ => panic!("Expected counter URef"),
    };

    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);
    let address_generator = make_address_generator();

    // Calling v1 vm directly by hash is not currently supported (i.e. disabling vm1 runtime, and
    // allowing vm1 direct calls may circumvent chainspec setting) let execute_request =
    // base_execute_builder()     .with_target(ExecutionKind::Stored {
    //         address: *counter_hash,
    //         entry_point: "counter_get".to_string(),
    //     })
    //     .with_input(runtime_args.into())
    //     .with_gas_limit(DEFAULT_GAS_LIMIT)
    //     .with_transferred_value(0)
    //     .with_shared_address_generator(Arc::clone(&address_generator))
    //     .with_state_hash(state_root_hash)
    //     .with_block_height(1)
    //     .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
    //     .build()
    //     .expect("should build");
    // let res = run_wasm_session(
    //     &mut executor,
    //     &mut global_state,
    //     state_root_hash,
    //     execute_request,
    // );
    // state_root_hash = global_state
    //     .commit_effects(state_root_hash, res.effects().clone())
    //     .expect("Should commit");

    //
    // Instantiate v2 runtime proxy contract
    //
    let input_data = counter_hash.to_vec();
    let install_request: InstallContractRequest = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2_counter_proxy.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data.into())
        .with_state_hash(state_root_hash)
        .with_block_height(2)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block2")))
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );

    state_root_hash = create_result.post_state_hash();

    let proxy_address = *create_result.smart_contract_addr();

    // Call v2 contract

    let call_request = base_execute_builder(&chainspec_config)
        .with_target(ExecutionKind::Stored {
            address: proxy_address,
            entry_point: "perform_test".to_string(),
        })
        .with_input(Bytes::new())
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_state_hash(state_root_hash)
        .with_block_height(3)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block3")))
        .build()
        .expect("should build");

    expect_successful_execution(&mut executor, &global_state, state_root_hash, call_request);
}

// host function tests

#[test]
fn host_functions_consume_gas() {
    fn assert_consumes_gas(chainspec_config: &ChainspecConfig, host_function_name: &str) {
        let result = call_dummy_host_fn_by_name(&chainspec_config.clone(), host_function_name, 1);
        assert!(result.is_err_and(|e| matches!(
            e,
            InstallContractError::Constructor {
                host_error: CallError::CalleeGasDepleted,
            }
        )));
    }

    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    assert_consumes_gas(&chainspec_config, "get_caller");
    assert_consumes_gas(&chainspec_config, "get_block_time");
    assert_consumes_gas(&chainspec_config, "get_transferred_value");
    assert_consumes_gas(&chainspec_config, "get_balance_of");
    assert_consumes_gas(&chainspec_config, "call");
    assert_consumes_gas(&chainspec_config, "input");
    assert_consumes_gas(&chainspec_config, "create");
    assert_consumes_gas(&chainspec_config, "print");
    assert_consumes_gas(&chainspec_config, "read");
    assert_consumes_gas(&chainspec_config, "ret");
    assert_consumes_gas(&chainspec_config, "transfer");
    assert_consumes_gas(&chainspec_config, "upgrade");
    assert_consumes_gas(&chainspec_config, "write");
    assert_consumes_gas(&chainspec_config, "generic_hash");
    assert_consumes_gas(&chainspec_config, "recover_secp256k1");
}

#[test]
fn non_existing_smart_contract_does_not_panic() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let address_generator = make_address_generator();
    let executor = make_executor(&chainspec_config);
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let non_existing_address = [255; 32];
    let execute_request = base_execute_builder(&chainspec_config)
        .with_target(ExecutionKind::Stored {
            address: non_existing_address,
            entry_point: "non_existing".to_string(),
        })
        .with_input(Bytes::new())
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .build()
        .expect("should build");

    let result = executor
        .execute_with_provider(state_root_hash, &global_state, execute_request)
        .expect_err("Failure");

    assert!(matches!(
        result,
        ExecuteWithProviderError::Execute(execute_error) if matches!(execute_error, ExecuteError::CodeNotFound(address) if address == non_existing_address)));
}

#[test]
fn casper_return_writes_to_execution_journal() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let address_generator = make_address_generator();
    let mut executor = make_executor(&chainspec_config);
    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    // Create a contract that will be used to test the ret host function
    let input_data = borsh::to_vec(&("write".to_string(),))
        .map(Bytes::from)
        .unwrap();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2_host.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data)
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );

    let contract_address = *create_result.smart_contract_addr();
    state_root_hash = create_result.post_state_hash();

    // Execute the contract to trigger the return
    let execute_request = base_execute_builder(&chainspec_config)
        .with_target(ExecutionKind::Stored {
            address: contract_address,
            entry_point: "ret".to_string(),
        })
        .with_input(Bytes::new())
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .build()
        .expect("should build");

    let execute_result = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    // Check that the effects contain a Ret transform
    let effects = execute_result.effects();
    let transforms = effects.transforms();

    let ret_transform = transforms.iter().find(|transform| {
        matches!(
            transform.kind(),
            casper_types::execution::TransformKindV2::Ret(_)
        )
    });

    assert!(
        ret_transform.is_some(),
        "Expected to find a Ret transform in the effects"
    );

    let ret_transform = ret_transform.unwrap();
    match ret_transform.kind() {
        casper_types::execution::TransformKindV2::Ret(RetValue::Bytes(bytes)) => {
            // The ret function in the test contract calls casper::ret with [1, 2, 3] data
            assert_eq!(
                bytes.as_slice(),
                &[1u8, 2, 3],
                "Return data should match what was passed to casper::ret"
            );
        }
        _ => panic!("Expected Ret transform kind"),
    }

    // Verify the key is the contract address
    let expected_key = Key::SmartContract(contract_address);
    assert_eq!(
        ret_transform.key(),
        &expected_key,
        "Ret transform should be under the contract key"
    );
}

#[test]
fn casper_return_fails_if_contract_uses_unsupported_flags() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");
    let address_generator = make_address_generator();
    let mut executor = make_executor(&chainspec_config);
    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    // Create a contract that will be used to test the ret host function
    let input_data = borsh::to_vec(&("write".to_string(),))
        .map(Bytes::from)
        .unwrap();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2_host.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data)
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );

    let contract_address = *create_result.smart_contract_addr();
    state_root_hash = create_result.post_state_hash();

    // Execute the contract to trigger the return
    let execute_request = base_execute_builder(&chainspec_config)
        .with_target(ExecutionKind::Stored {
            address: contract_address,
            entry_point: "ret_faulty_flags".to_string(),
        })
        .with_input(Bytes::new())
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .build()
        .expect("should build");

    let result = executor.execute_with_provider(state_root_hash, &global_state, execute_request);
    assert!(result.is_err());
    let err: ExecuteWithProviderError = result.expect_err("should have error details");
    assert!(matches!(
        err,
        ExecuteWithProviderError::Execute(ExecuteError::ReturnFlagsNotSupported(2))
    ));
}

#[test]

fn escrow() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");
    let mut executor = make_executor(&chainspec_config);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let input_data = Bytes::new();
    let block_time_1 = Timestamp::now().into();

    let create_request = base_install_request_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(read_wasm("vm2_escrow.wasm").clone())
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data)
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
        create_request,
    );

    let contract_hash = create_result.smart_contract_addr();

    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::Stored {
            address: *contract_hash,
            entry_point: "deposit_tokens".to_string(),
        })
        .with_serialized_input(())
        .expect("expected serialized input to be correct")
        .with_transferred_value(10000)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(1234567890.into())
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(2) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .build()
        .expect("should build");

    let result_2 = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    )
    .expect("should have result");

    let post_state_root_hash = global_state
        .commit_effects(state_root_hash, result_2.effects().clone())
        .expect("Should commit");

    assert_ne!(post_state_root_hash, state_root_hash);
}

#[test]
fn should_not_fail_without_account() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");
    let executor = make_executor(&chainspec_config);

    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let input_data = Bytes::new();
    let block_time_1 = Timestamp::now().into();

    let create_request = base_install_request_builder(&chainspec_config)
        .with_initiator(AccountHash::new([0xF0; 32]))
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(read_wasm("vm2_escrow.wasm").clone())
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data)
        .with_block_time(block_time_1)
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(1) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .build()
        .expect("should build");

    let _create_result = {
        executor
            .install_contract(state_root_hash, &global_state, create_request)
            .expect_err("Succeed")
    };
}

#[test]

fn supports_named_args_convention() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");
    let mut executor = make_executor(&chainspec_config);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let input_data = {
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("value".to_string(), 123u32).unwrap();
        runtime_args.to_bytes().map(Bytes::from).unwrap()
    };

    let block_time_1 = Timestamp::now().into();

    let create_request = base_install_request_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(read_wasm("vm2_named_args.wasm").clone())
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data)
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
        create_request,
    );

    let contract_hash = create_result.smart_contract_addr();

    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::Stored {
            address: *contract_hash,
            entry_point: "deposit_tokens".to_string(),
        })
        .with_serialized_input(())
        .unwrap()
        .with_transferred_value(10000)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(1234567890.into())
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(2) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .build()
        .expect("should build");

    let result_2 = run_wasm_session(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    )
    .expect("should have result");

    let post_state_root_hash = global_state
        .commit_effects(state_root_hash, result_2.effects().clone())
        .expect("Should commit");

    assert_ne!(post_state_root_hash, state_root_hash);
}
