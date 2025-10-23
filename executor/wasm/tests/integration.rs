use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use casper_execution_engine::runtime::cryptography;
use casper_executor_wasm::{
    install::{InstallContractError, InstallContractRequest},
    testing::{
        base_execute_builder, base_install_request_builder, call_dummy_host_fn_by_name,
        expect_successful_execution, make_address_generator, make_executor,
        make_global_state_with_genesis, make_runtime_config, read_wasm, run_create_contract,
        run_wasm_session, DEFAULT_GAS_LIMIT, TRANSACTION_HASH,
    },
    ExecutorV2,
};

use casper_executor_wasm::{
    chainspec_config,
    chainspec_config::{ChainspecConfig, DEFAULT_ACCOUNT_HASH},
    testing::DEFAULT_STABLE_VALIDATOR_PUBLIC_KEY,
};
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::executor::{
    AuctionMethods, ExecuteError, ExecuteRequest, ExecuteWithProviderError, ExecutionKind, FFIMenu,
    MintMethods,
};

use casper_executor_wasm::testing::{DEFAULT_CHAIN_NAME, DEFAULT_STABLE_DELEGATOR_PUBLIC_KEY};
use casper_storage::{
    data_access_layer::{
        prefixed_values::{PrefixedValuesRequest, PrefixedValuesResult},
        tagged_values::{TaggedValuesRequest, TaggedValuesResult, TaggedValuesSelection},
        MessageTopicsRequest, MessageTopicsResult, QueryRequest, QueryResult,
    },
    global_state::{
        state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
        transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
    AddressGenerator, KeyPrefix,
};

use casper_types::{
    account::AccountHash,
    bytesrepr::ToBytes,
    contract_messages::{Message, MessageChecksum, MessagePayload},
    execution::RetValue,
    system::auction::{BidAddr, BidKind},
    BlockHash, BlockTime, ByteCodeAddr, Digest, EntityAddr, Key, KeyTag, PublicKey, RuntimeArgs,
    StoredValue, Timestamp,
};
use fs_extra::dir;
use itertools::Itertools;
use once_cell::sync::Lazy;
use parking_lot::RwLock;

const VM2_SYSTEM_CALLER_WASM: &str = "vm2_system_caller.wasm";

/// Symlink to chainspec.
pub static CHAINSPEC_SYMLINK: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/")
        .join(chainspec_config::CHAINSPEC_NAME)
});

#[test]
fn vm2_should_return_output_to_caller() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);
    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2-harness.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("initialize".to_string())
        .with_input(Bytes::from(b"".as_slice()))
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );

    let contract_hash = EntityAddr::SmartContract(*create_result.smart_contract_addr());
    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let input = borsh::to_vec(&(String::from("hi"),))
        .map(Bytes::from)
        .unwrap();

    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_hash.value(),
            entry_point: "entry_point_without_state_with_args_and_output".to_string(),
        })
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(2)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block")))
        .with_input(input)
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .build()
        .expect("should build");

    let result = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );
    assert!(result.host_error.is_none(), "should be success");
    let out = result.output().expect("should have output");
    let returned: String = borsh::from_slice(out).expect("borsh string");
    assert_eq!(returned, "hiextra");
}

#[test]
fn vm2_rollback_should_return_to_caller_with_data() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);
    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2-harness.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("initialize".to_string())
        .with_input(Bytes::from(b"".as_slice()))
        .build()
        .expect("should build");
    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );
    let contract_hash = EntityAddr::SmartContract(*create_result.smart_contract_addr());
    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let input = borsh::to_vec(&()).map(Bytes::from).unwrap();
    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_hash.value(),
            entry_point: "emit_revert_with_data".to_string(),
        })
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(2)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"bl0ck")))
        .with_input(input)
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .build()
        .expect("should build");

    let result = executor
        .execute_with_provider(state_root_hash, &global_state, execute_request)
        .expect("exec ok");
    match result.host_error {
        Some(CallError::CalleeRolledBack) => {}
        Some(other) => panic!("expected CalleeRolledBack got {other:?}"),
        None => panic!("expected error"),
    }

    let out = result.output().expect("should carry rollback data");
    assert!(!out.is_empty(), "rollback should return some data");
}

#[test]
fn vm2_revert_should_abort_whole_stack() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);
    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2-harness.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("initialize".to_string())
        .with_input(Bytes::from(b"".as_slice()))
        .build()
        .expect("should build");
    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );
    let contract_hash = EntityAddr::SmartContract(*create_result.smart_contract_addr());
    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let input = borsh::to_vec(&()).map(Bytes::from).unwrap();
    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_hash.value(),
            entry_point: "emit_revert_without_data".to_string(),
        })
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(2)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"bl0ck")))
        .with_input(input)
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .build()
        .expect("should build");

    let result = executor
        .execute_with_provider(state_root_hash, &global_state, execute_request)
        .expect("exec ok");
    match result.host_error {
        Some(CallError::Api(_)) => {}
        Some(other) => panic!("expected Api(_) got {other:?}"),
        None => panic!("expected error"),
    }
}

fn harness() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let cep18_address;

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

        cep18_address = *create_result.smart_contract_addr();

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
        .with_execution_kind(ExecutionKind::SessionBytes(read_wasm("vm2-harness.wasm")))
        .with_serialized_input((cep18_address,))
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

fn make_execution_request(
    chainspec_config: &ChainspecConfig,
    address_generator: Arc<RwLock<AddressGenerator>>,
    execution_kind: ExecutionKind,
    input_data: Bytes,
    transferred_value: u64,
    initiator: Option<AccountHash>,
    caller_key: Option<Key>,
    block_time: Option<BlockTime>,
) -> ExecuteRequest {
    let account_hash = initiator.unwrap_or(*DEFAULT_ACCOUNT_HASH);
    let caller_key = caller_key.unwrap_or(Key::Account(account_hash));
    let block_time = block_time.unwrap_or(Timestamp::now().into());
    base_execute_builder(chainspec_config)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_runtime_native_config(make_runtime_config(chainspec_config))
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_block_time(block_time)
        .with_initiator(account_hash)
        .with_caller_key(caller_key)
        .with_execution_kind(execution_kind)
        .with_transferred_value(transferred_value)
        .with_input(input_data)
        .build()
        .expect("should build")
}

fn exec_system_call(system_menu: FFIMenu, initiator: Option<AccountHash>) {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config")
        .with_vesting_schedule_period_millis(0);

    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();
    let block_time = Timestamp::now().into();
    let initiator = initiator.unwrap_or(DEFAULT_STABLE_VALIDATOR_PUBLIC_KEY.to_account_hash());

    let system_function_option: u32 = system_menu.into();
    let input_data = borsh::to_vec(&(system_function_option, false))
        .map(Bytes::from)
        .unwrap();
    let execute_request = make_execution_request(
        &chainspec_config,
        Arc::clone(&address_generator),
        ExecutionKind::SessionBytes(read_wasm(VM2_SYSTEM_CALLER_WASM)),
        input_data,
        0,
        Some(initiator),
        None,
        Some(block_time),
    );

    let executor = make_executor(&chainspec_config);
    let result = executor.execute_with_provider(state_root_hash, &global_state, execute_request);

    match result {
        Ok(result) => {
            if let Some(host_error) = result.host_error {
                panic!("Host error: {host_error:?}");
            }
        }
        Err(err) => panic!("Host error: {err:?}"),
    }
}

fn exec_and_commit(
    executor: &ExecutorV2,
    global_state: &LmdbGlobalState,
    state_root_hash: &Digest,
    request: ExecuteRequest,
) -> Result<Digest, String> {
    let pre_state = *state_root_hash;
    match executor.execute_with_provider(pre_state, global_state, request) {
        Ok(result) => {
            let host_error = &result.host_error;
            if let Some(host_error) = host_error {
                Err(format!("Host error: {host_error:?}"))
            } else {
                match global_state.commit_effects(pre_state, result.effects().clone()) {
                    Ok(post_state) => Ok(post_state),
                    Err(gs_err) => {
                        let msg = format!("GS error: {gs_err:?}");
                        Err(msg)
                    }
                }
            }
        }
        Err(ex_err) => {
            let msg = format!("Exec error: {ex_err:?}");
            Err(msg)
        }
    }
}

#[test]
fn should_revert_invalid_system_option() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();
    let address_generator = make_address_generator();

    let block_time = Timestamp::now().into();

    let account_hash = DEFAULT_STABLE_VALIDATOR_PUBLIC_KEY.to_account_hash();
    let input_data = borsh::to_vec(&(9999, false)).map(Bytes::from).unwrap();

    let execute_request = make_execution_request(
        &chainspec_config,
        Arc::clone(&address_generator),
        ExecutionKind::SessionBytes(read_wasm(VM2_SYSTEM_CALLER_WASM)),
        input_data,
        0,
        Some(account_hash),
        None,
        Some(block_time),
    );

    let executor = make_executor(&chainspec_config);

    let result = executor.execute_with_provider(state_root_hash, &global_state, execute_request);

    if let Ok(exec_result) = result {
        assert!(exec_result.host_error.is_some(), "should have error");
        match exec_result.host_error {
            Some(CallError::CalleeRolledBack) => {
                // noop, expected outcome
            }
            Some(err) => {
                panic!("expected: CalleeRolledBack actual: {}", err);
            }
            None => {
                panic!("should have error")
            }
        }
    }
}

#[test]
fn should_call_system_transfer() {
    exec_system_call(FFIMenu::Mint(MintMethods::Transfer), None);
}

#[test]
fn should_call_system_transfer_purse() {
    exec_system_call(FFIMenu::Mint(MintMethods::TransferPurse), None);
}

#[test]
fn should_call_system_burn() {
    exec_system_call(FFIMenu::Mint(MintMethods::Burn), None);
}

#[test]
fn should_call_system_activate_bid() {
    exec_system_call(FFIMenu::Auction(AuctionMethods::Activate), None);
}

#[test]
fn should_call_system_bid() {
    exec_system_call(FFIMenu::Auction(AuctionMethods::Bid), None);
}

#[test]
fn should_call_system_withdraw() {
    exec_system_call(FFIMenu::Auction(AuctionMethods::Withdraw), None);
}

#[test]
fn should_call_system_change_public_key() {
    exec_system_call(FFIMenu::Auction(AuctionMethods::ChangePublicKey), None);
}

#[test]
fn should_call_system_delegate() {
    exec_system_call(
        FFIMenu::Auction(AuctionMethods::Delegate),
        Some(DEFAULT_STABLE_DELEGATOR_PUBLIC_KEY.to_account_hash()),
    );
}

#[test]
fn should_call_system_undelegate() {
    exec_system_call(
        FFIMenu::Auction(AuctionMethods::Undelegate),
        Some(DEFAULT_STABLE_DELEGATOR_PUBLIC_KEY.to_account_hash()),
    );
}

#[test]
fn should_call_system_redelegate() {
    exec_system_call(
        FFIMenu::Auction(AuctionMethods::Redelegate),
        Some(DEFAULT_STABLE_DELEGATOR_PUBLIC_KEY.to_account_hash()),
    );
}

// this test handles both add and cancel reservations
#[test]
fn should_handle_reservations() {
    // this test is more complicated than the other system functions
    // there is no way to set non-zero reservation slots on genesis bids
    // thus they are defaulted to 0.
    //
    // so, to get a full test across the entire feature, we need to:
    //  1) upsert a validator bid to allow some number of reservations
    //  2) then add reservations
    //  3) then cancel those same reservations
    //
    // we also must apply the changes to global state between the steps

    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config")
        .with_vesting_schedule_period_millis(0);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();
    let block_time = Timestamp::now().into();
    let account_hash = DEFAULT_STABLE_VALIDATOR_PUBLIC_KEY.to_account_hash();
    let executor = make_executor(&chainspec_config);

    // need to bump the delegator reservation limit up to allow add_reservation to work
    let bid_request = {
        let opt: u32 = FFIMenu::Auction(AuctionMethods::Bid).into();
        let input_data = borsh::to_vec(&(opt, false)).map(Bytes::from).unwrap();
        make_execution_request(
            &chainspec_config,
            Arc::clone(&address_generator),
            ExecutionKind::SessionBytes(read_wasm(VM2_SYSTEM_CALLER_WASM)),
            input_data,
            0,
            Some(account_hash),
            None,
            Some(block_time),
        )
    };

    state_root_hash = match exec_and_commit(&executor, &global_state, &state_root_hash, bid_request)
    {
        Ok(post_state) => post_state,
        Err(err_str) => panic!("{err_str}"),
    };

    // make sure the bid was updated to allow reservations
    match global_state.query(QueryRequest::new(
        state_root_hash,
        Key::BidAddr(BidAddr::Validator(account_hash)),
        vec![],
    )) {
        QueryResult::Success { value, .. } => {
            if let StoredValue::BidKind(BidKind::Validator(validator)) = *value {
                assert_eq!(validator.reserved_slots(), 2, "expected 2 slots");
            } else {
                panic!("should have validator bid")
            }
        }
        _ => panic!("expected validator bid"),
    }

    // make a couple of reservations
    let add_res_pubk_request = {
        let opt: u32 = FFIMenu::Auction(AuctionMethods::AddReservation).into();
        let input_data = borsh::to_vec(&(opt, false)).map(Bytes::from).unwrap();
        make_execution_request(
            &chainspec_config,
            Arc::clone(&address_generator),
            ExecutionKind::SessionBytes(read_wasm(VM2_SYSTEM_CALLER_WASM)),
            input_data,
            0,
            Some(account_hash),
            None,
            Some(block_time),
        )
    };

    state_root_hash = match exec_and_commit(
        &executor,
        &global_state,
        &state_root_hash,
        add_res_pubk_request,
    ) {
        Ok(post_state) => post_state,
        Err(err_str) => panic!("{err_str}"),
    };

    let add_res_purse_request = {
        let opt: u32 = FFIMenu::Auction(AuctionMethods::AddReservation).into();
        let input_data = borsh::to_vec(&(opt, true)).map(Bytes::from).unwrap();
        make_execution_request(
            &chainspec_config,
            Arc::clone(&address_generator),
            ExecutionKind::SessionBytes(read_wasm(VM2_SYSTEM_CALLER_WASM)),
            input_data,
            0,
            Some(account_hash),
            None,
            Some(block_time),
        )
    };

    state_root_hash = match exec_and_commit(
        &executor,
        &global_state,
        &state_root_hash,
        add_res_purse_request,
    ) {
        Ok(post_state) => post_state,
        Err(err_str) => panic!("{err_str}"),
    };

    // cancel those reservations
    let cancel_pubk_request = {
        let opt: u32 = FFIMenu::Auction(AuctionMethods::CancelReservation).into();
        let input_data = borsh::to_vec(&(opt, false)).map(Bytes::from).unwrap();
        make_execution_request(
            &chainspec_config,
            Arc::clone(&address_generator),
            ExecutionKind::SessionBytes(read_wasm(VM2_SYSTEM_CALLER_WASM)),
            input_data,
            0,
            Some(account_hash),
            None,
            Some(block_time),
        )
    };

    match exec_and_commit(
        &executor,
        &global_state,
        &state_root_hash,
        cancel_pubk_request,
    ) {
        Ok(post_state) => post_state,
        Err(err_str) => panic!("{err_str}"),
    };

    let cancel_purse_request = {
        let opt: u32 = FFIMenu::Auction(AuctionMethods::CancelReservation).into();
        let input_data = borsh::to_vec(&(opt, true)).map(Bytes::from).unwrap();
        make_execution_request(
            &chainspec_config,
            Arc::clone(&address_generator),
            ExecutionKind::SessionBytes(read_wasm(VM2_SYSTEM_CALLER_WASM)),
            input_data,
            0,
            Some(account_hash),
            None,
            Some(block_time),
        )
    };

    match exec_and_commit(
        &executor,
        &global_state,
        &state_root_hash,
        cancel_purse_request,
    ) {
        Ok(post_state) => post_state,
        Err(err_str) => panic!("{err_str}"),
    };
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
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
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
        .with_execution_kind(ExecutionKind::SessionBytes(read_wasm(
            "vm2_cep18_caller.wasm",
        )))
        .with_serialized_input((create_result.smart_contract_addr(),))
        .expect("expected serialized input to be correct")
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(block_time_2)
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(2)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
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
fn counter() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let block_time_1 = Timestamp::now().into();

    let create_request = base_install_request_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(read_wasm("vm2_counter.wasm").clone())
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("default".to_string())
        .with_block_time(block_time_1)
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
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

    let query_request = QueryRequest::new(state_root_hash, Key::State(contract_hash), vec![]);
    match global_state.query(query_request) {
        QueryResult::RootNotFound | QueryResult::ValueNotFound(_) | QueryResult::Failure(_) => {
            panic!("query failed");
        }
        QueryResult::Success { value, .. } => {
            if let StoredValue::CLValue(cl_value) = *value {
                let counter: (u32,) =
                    borsh::from_slice(cl_value.inner_bytes()).expect("should deserialize");
                assert_eq!(counter.0, 0u32, "should be 0");
            } else {
                println!("{:?}", value);
                panic!("wrong stored value variant");
            }
        }
    }

    let block_time_2 = (block_time_1.value() + 1).into();
    assert_ne!(block_time_1, block_time_2);

    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_hash.value(),
            entry_point: "increment".to_string(),
        })
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(block_time_2)
        .with_state_hash(state_root_hash)
        .with_block_height(2)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([1; 32])))
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .build()
        .expect("should build");

    let result_2 = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    assert!(result_2.host_error.is_none(), "increment should work");

    state_root_hash = global_state
        .commit_effects(state_root_hash, result_2.effects().clone())
        .expect("Should commit");

    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_hash.value(),
            entry_point: "get".to_string(),
        })
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(block_time_2)
        .with_state_hash(state_root_hash)
        .with_block_height(3)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([2; 32])))
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .build()
        .expect("should build");

    let result_get = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    match result_get.output() {
        Some(bytes) => {
            let count: u32 = borsh::from_slice(bytes).expect("should deserialize");
            assert_eq!(count, 1u32, "get should return the current count of 1");
        }
        None => panic!("get should have output"),
    }

    let query_request = QueryRequest::new(state_root_hash, Key::State(contract_hash), vec![]);
    match global_state.query(query_request) {
        QueryResult::RootNotFound | QueryResult::ValueNotFound(_) | QueryResult::Failure(_) => {
            panic!("query failed");
        }
        QueryResult::Success { value, .. } => {
            if let StoredValue::CLValue(cl_value) = *value {
                let counter: (u32,) =
                    borsh::from_slice(cl_value.inner_bytes()).expect("should deserialize");
                assert_eq!(counter.0, 1u32, "should be 1");
            } else {
                println!("{:?}", value);
                panic!("wrong stored value variant");
            }
        }
    }

    let block_time_3 = (block_time_2.value() + 1).into();
    assert_ne!(block_time_2, block_time_3);

    let execute_request = base_execute_builder(&chainspec_config)
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_hash.value(),
            entry_point: "decrement".to_string(),
        })
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(block_time_3)
        .with_state_hash(state_root_hash)
        .with_block_height(3)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([2; 32])))
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .build()
        .expect("should build");

    let result_3 = expect_successful_execution(
        &mut executor,
        &global_state,
        state_root_hash,
        execute_request,
    );

    assert!(result_3.host_error.is_none(), "decrement should work");

    state_root_hash = global_state
        .commit_effects(state_root_hash, result_3.effects().clone())
        .expect("Should commit");

    let query_request = QueryRequest::new(state_root_hash, Key::State(contract_hash), vec![]);
    match global_state.query(query_request) {
        QueryResult::RootNotFound | QueryResult::ValueNotFound(_) | QueryResult::Failure(_) => {
            panic!("query failed");
        }
        QueryResult::Success { value, .. } => {
            if let StoredValue::CLValue(cl_value) = *value {
                let counter: (u32,) =
                    borsh::from_slice(cl_value.inner_bytes()).expect("should deserialize");
                assert_eq!(counter.0, 0u32, "should be 0");
            } else {
                println!("{:?}", value);
                panic!("wrong stored value variant");
            }
        }
    }
}

#[test]
fn traits() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);
    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let execute_request = base_execute_builder(&chainspec_config)
        .with_execution_kind(ExecutionKind::SessionBytes(read_wasm("vm2_trait.wasm")))
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
            .with_execution_kind(ExecutionKind::Stored {
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
            .with_execution_kind(ExecutionKind::Stored {
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
        .with_execution_kind(ExecutionKind::Stored {
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
            .with_execution_kind(ExecutionKind::Stored {
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
            .with_execution_kind(ExecutionKind::Stored {
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

#[ignore]
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

    let value = result.as_value().expect("should have value");

    //
    // Calling VM1 contract directly by its address
    //

    let mut state_root_hash = post_state_hash;

    let value = match value {
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

    //
    // Instantiate v2 runtime proxy contract
    //
    let input_data = counter_hash.to_vec();
    let install_request: InstallContractRequest = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2_vm1_wrapper.wasm"))
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
        .with_execution_kind(ExecutionKind::Stored {
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
        .with_execution_kind(ExecutionKind::Stored {
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
        .expect("should return execute with call error")
        .host_error;

    assert!(matches!(result, Some(CallError::CodeNotFound)))
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
        .with_execution_kind(ExecutionKind::Stored {
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
    let expected_key = if chainspec_config.core_config.addressable_entity_enabled {
        Key::AddressableEntity(EntityAddr::SmartContract(contract_address))
    } else {
        Key::Hash(contract_address)
    };
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
        .with_execution_kind(ExecutionKind::Stored {
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
    match err {
        ExecuteWithProviderError::Execute(ExecuteError::ReturnFlagsNotSupported(v)) => {
            assert!(v != 0, "invalid flags should be non-zero");
        }
        other => panic!("expected ReturnFlagsNotSupported, got {other:?}"),
    }
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
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
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
        .with_execution_kind(ExecutionKind::Stored {
            address: *contract_hash,
            entry_point: "deposit_tokens".to_string(),
        })
        .with_serialized_input(())
        .expect("expected serialized input to be correct")
        .with_transferred_value(10000)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(1234567890.into())
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(2)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
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
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
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
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
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
        .with_execution_kind(ExecutionKind::Stored {
            address: *contract_hash,
            entry_point: "deposit_tokens".to_string(),
        })
        .with_serialized_input(())
        .unwrap()
        .with_transferred_value(10000)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_block_time(1234567890.into())
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(2)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
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
fn installing_contract_should_produce_system_messages() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");
    let is_addressable_entity_enabled = chainspec_config.core_config.addressable_entity_enabled;

    let mut executor = make_executor(&chainspec_config);

    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();
    let input_data = borsh::to_vec(&(0u8,)).map(Bytes::from).unwrap();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2_upgradable.wasm"))
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
    let post_state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");
    let request = TaggedValuesRequest::new(
        post_state_root_hash,
        TaggedValuesSelection::All(KeyTag::Message),
    );
    let message_checksums: Vec<MessageChecksum> = as_values(global_state.tagged_values(request))
        .unwrap()
        .into_iter()
        .filter_map(|stored_value| match stored_value {
            StoredValue::Message(message_checksum) => Some(message_checksum),
            _ => None,
        })
        .collect();
    assert_eq!(message_checksums.len(), 4);
    let key_of_contract = if is_addressable_entity_enabled {
        Key::AddressableEntity(EntityAddr::SmartContract(
            *create_result.smart_contract_addr(),
        ))
    } else {
        Key::Hash(*create_result.smart_contract_addr())
    };
    let (key_of_contract, key_of_package, key_of_wasm) =
        get_contract_package_and_wasms(post_state_root_hash, &global_state, key_of_contract);

    let system_account_hash = PublicKey::System.to_account_hash().value();
    let entity_addr = EntityAddr::Account(system_account_hash);
    expect_message_on_topic_and_index(
        post_state_root_hash,
        &global_state,
        &key_of_package.to_formatted_string(),
        "package_key",
        entity_addr,
        0,
        0,
    );
    expect_message_on_topic_and_index(
        post_state_root_hash,
        &global_state,
        &key_of_contract.to_formatted_string(),
        "contract_key",
        entity_addr,
        1,
        0,
    );
    expect_message_on_topic_and_index(
        post_state_root_hash,
        &global_state,
        &key_of_wasm.to_formatted_string(),
        "bytecode_key",
        entity_addr,
        2,
        0,
    );
    expect_message_on_topic_and_index(
        post_state_root_hash,
        &global_state,
        &format!("{}.{}", 2, 1),
        "contract_version",
        entity_addr,
        3,
        0,
    );
}

#[test]
fn installing_contract_should_produce_system_messages_after_upgrade() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");
    let is_addressable_entity_enabled = chainspec_config.core_config.addressable_entity_enabled;

    let mut executor = make_executor(&chainspec_config);
    let upgradable_address;
    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();
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
    let binding = read_wasm("vm2_upgradable_v2.wasm");
    let new_code = binding.as_ref();

    let execute_request = base_execute_builder(&chainspec_config)
        .with_transferred_value(0)
        .with_execution_kind(ExecutionKind::Stored {
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
    let state_root_hash_after_upgrade = global_state
        .commit_effects(state_root_hash, res.effects().clone())
        .expect("Should commit");

    let request = TaggedValuesRequest::new(
        state_root_hash_after_upgrade,
        TaggedValuesSelection::All(KeyTag::Message),
    );
    let message_checksums: Vec<MessageChecksum> = as_values(global_state.tagged_values(request))
        .unwrap()
        .into_iter()
        .filter_map(|stored_value| match stored_value {
            StoredValue::Message(message_checksum) => Some(message_checksum),
            _ => None,
        })
        .collect();

    assert_eq!(message_checksums.len(), 4);
    let key_of_contract = if is_addressable_entity_enabled {
        Key::AddressableEntity(EntityAddr::SmartContract(upgradable_address))
    } else {
        Key::Hash(upgradable_address)
    };
    let (key_of_contract, key_of_package, key_of_wasm) = get_contract_package_and_wasms(
        state_root_hash_after_upgrade,
        &global_state,
        key_of_contract,
    );

    let system_account_hash = PublicKey::System.to_account_hash().value();
    let entity_addr = EntityAddr::Account(system_account_hash);
    expect_message_on_topic_and_index(
        state_root_hash_after_upgrade,
        &global_state,
        &key_of_package.to_formatted_string(),
        "package_key",
        entity_addr,
        0,
        0,
    );
    expect_message_on_topic_and_index(
        state_root_hash_after_upgrade,
        &global_state,
        &key_of_contract.to_formatted_string(),
        "contract_key",
        entity_addr,
        1,
        0,
    );
    expect_message_on_topic_and_index(
        state_root_hash_after_upgrade,
        &global_state,
        &key_of_wasm.to_formatted_string(),
        "bytecode_key",
        entity_addr,
        2,
        0,
    );
    expect_message_on_topic_and_index(
        state_root_hash_after_upgrade,
        &global_state,
        &format!("{}.{}", 2, 2),
        "contract_version",
        entity_addr,
        3,
        0,
    );
}

fn get_contract_package_and_wasms(
    state_hash: Digest,
    global_state: &LmdbGlobalState,
    key_of_contract: Key,
) -> (Key, Key, Key) {
    let mut tc = global_state.tracking_copy(state_hash).unwrap().unwrap();
    let z = tc.read(&key_of_contract).unwrap().unwrap();
    match z {
        StoredValue::AddressableEntity(ae) => (
            key_of_contract,
            Key::Package(ae.package()),
            Key::ByteCode(ae.byte_code_addr().unwrap()),
        ),
        StoredValue::Contract(ctr) => (
            key_of_contract,
            Key::Hash(ctr.contract_package_hash().value()),
            //TODO this should probably be changed to ctr.contract_wasm_key() once the upgrade is
            // fixed
            Key::ByteCode(ByteCodeAddr::V2CasperWasm(ctr.contract_wasm_hash().value())),
        ),
        StoredValue::ContractPackage(contract_package) => {
            let real_contract_entry =
                Key::Hash(contract_package.current_contract_hash().unwrap().value());
            let z = tc.read(&real_contract_entry).unwrap().unwrap();
            let wasm_key = match z {
                StoredValue::Contract(ctr) => {
                    //TODO this should probably be changed to ctr.contract_wasm_key() once the
                    // upgrade is fixed
                    Key::ByteCode(ByteCodeAddr::V2CasperWasm(ctr.contract_wasm_hash().value()))
                }
                _ => unreachable!(),
            };
            (real_contract_entry, key_of_contract, wasm_key)
        }
        _ => unreachable!(),
    }
}

fn as_values(res: TaggedValuesResult) -> Option<Vec<StoredValue>> {
    match res {
        TaggedValuesResult::Success {
            selection: _,
            values,
        } => Some(values),
        _ => None,
    }
}

fn expect_message_on_topic_and_index(
    state_hash: Digest,
    global_state: &LmdbGlobalState,
    message: &str,
    topic_name: &str,
    entity_addr: EntityAddr,
    index_in_block: u64,
    index_in_topic: u32,
) {
    let topic_name_hash = cryptography::blake2b(topic_name);
    let key = Key::message(entity_addr, topic_name_hash.into(), index_in_topic);
    let res = global_state.query(QueryRequest::new(state_hash, key, vec![]));
    let got_message_checksum = match res {
        QueryResult::Success { value, proofs: _ } => value.as_message_checksum().unwrap().clone(),
        _ => unreachable!(),
    };
    let message = Message::new(
        entity_addr,
        MessagePayload::String(message.to_string()),
        topic_name.to_string(),
        topic_name_hash.into(),
        index_in_topic,
        index_in_block,
    );
    let message_checksum = message.checksum().unwrap();
    assert_eq!(got_message_checksum, message_checksum);
}
