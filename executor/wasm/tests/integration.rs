use std::{fs::File, path::Path, sync::Arc};

use bytes::Bytes;
use casper_executor_wasm::{
    install::{InstallContractRequest, InstallContractRequestBuilder, InstallContractResult},
    ExecutorConfigBuilder, ExecutorKind, ExecutorV2,
};
use casper_executor_wasm_interface::executor::{
    ExecuteRequest, ExecuteRequestBuilder, ExecuteWithProviderResult, ExecutionKind,
};
use casper_storage::{
    data_access_layer::{GenesisRequest, GenesisResult, QueryRequest, QueryResult},
    global_state::{
        self,
        state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
        transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
    system::runtime_native::Id,
    AddressGenerator,
};
use casper_types::{
    account::AccountHash, bytesrepr::ToBytes, ChainspecRegistry, Digest, EntityAddr,
    GenesisAccount, GenesisConfigBuilder, Key, Motes, Phase, ProtocolVersion, PublicKey,
    RuntimeArgs, SecretKey, StoredValue, Timestamp, TransactionHash, TransactionV1Hash, U512,
};
use fs_extra::dir;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use tempfile::TempDir;

static DEFAULT_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap());
static DEFAULT_ACCOUNT_PUBLIC_KEY: Lazy<casper_types::PublicKey> =
    Lazy::new(|| PublicKey::from(&*DEFAULT_ACCOUNT_SECRET_KEY));
static DEFAULT_ACCOUNT_HASH: Lazy<AccountHash> =
    Lazy::new(|| DEFAULT_ACCOUNT_PUBLIC_KEY.to_account_hash());

const CSPR: u64 = 10u64.pow(9);

// const VM2_TEST_CONTRACT: Bytes =
// Bytes::from_static(include_bytes!("../vqm2-test-contract.wasm"));
const VM2_HARNESS: Bytes = Bytes::from_static(include_bytes!("../vm2-harness.wasm"));
const VM2_CEP18: Bytes = Bytes::from_static(include_bytes!("../vm2_cep18.wasm"));
const VM2_LEGACY_COUNTER_PROXY: Bytes =
    Bytes::from_static(include_bytes!("../vm2_legacy_counter_proxy.wasm"));
const VM2_CEP18_CALLER: Bytes = Bytes::from_static(include_bytes!("../vm2-cep18-caller.wasm"));
const VM2_TRAIT: Bytes = Bytes::from_static(include_bytes!("../vm2_trait.wasm"));
// const VM2_FLIPPER: Bytes = Bytes::from_static(include_bytes!("../vm2_flipper.wasm"));
const VM2_UPGRADABLE: Bytes = Bytes::from_static(include_bytes!("../vm2_upgradable.wasm"));
const VM2_UPGRADABLE_V2: Bytes = Bytes::from_static(include_bytes!("../vm2_upgradable_v2.wasm"));

const TRANSACTION_HASH_BYTES: [u8; 32] = [55; 32];
const TRANSACTION_HASH: TransactionHash =
    TransactionHash::V1(TransactionV1Hash::from_raw(TRANSACTION_HASH_BYTES));

const DEFAULT_GAS_LIMIT: u64 = 1_000_000_000_000;
const DEFAULT_CHAIN_NAME: &str = "casper-test";

fn make_address_generator() -> Arc<RwLock<AddressGenerator>> {
    let id = Id::Transaction(TRANSACTION_HASH);
    Arc::new(RwLock::new(AddressGenerator::new(
        &id.seed(),
        Phase::Session,
    )))
}

fn base_execute_builder() -> ExecuteRequestBuilder {
    ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_callee_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(1000)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now())
}

fn base_install_request_builder() -> InstallContractRequestBuilder {
    InstallContractRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_gas_limit(1_000_000)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now())
}

// #[test]
// fn test_contract() {
//     let mut executor = make_executor();

//     let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

//     let input = ("Hello, world!".to_string(), 123456789u32);

//     let address_generator = make_address_generator();

//     let execute_request = base_execute_builder()
//         .with_target(ExecutionKind::SessionBytes(VM2_TEST_CONTRACT))
//         .with_serialized_input(input)
//         .with_shared_address_generator(address_generator)
//         .build()
//         .expect("should build");

//     let _effects = run_wasm_session(
//         &mut executor,
//         &mut global_state,
//         state_root_hash,
//         execute_request,
//     );
// }

#[test]
fn harness() {
    let mut executor = make_executor();

    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let flipper_address;

    state_root_hash = {
        let input_data = borsh::to_vec(&("Foo Token".to_string(),))
            .map(Bytes::from)
            .unwrap();

        let install_request = base_install_request_builder()
            .with_wasm_bytes(VM2_CEP18.clone())
            .with_shared_address_generator(Arc::clone(&address_generator))
            .with_transferred_value(0)
            .with_entry_point("new".to_string())
            .with_input(input_data)
            .build()
            .expect("should build");

        let create_result = run_create_contract(
            &mut executor,
            &mut global_state,
            state_root_hash,
            install_request,
        );

        flipper_address = create_result.contract_hash().value();

        global_state
            .commit(state_root_hash, create_result.effects().clone())
            .expect("Should commit")
    };

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_callee_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(1000)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(VM2_HARNESS))
        .with_serialized_input((flipper_address,))
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now())
        .build()
        .expect("should build");
    run_wasm_session(
        &mut executor,
        &mut global_state,
        state_root_hash,
        execute_request,
    );
}

fn make_executor() -> ExecutorV2 {
    let executor_config = ExecutorConfigBuilder::default()
        .with_memory_limit(17)
        .with_executor_kind(ExecutorKind::Compiled)
        .build()
        .expect("Should build");
    ExecutorV2::new(executor_config)
}

#[test]
fn cep18() {
    let mut executor = make_executor();

    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let input_data = borsh::to_vec(&("Foo Token".to_string(),))
        .map(Bytes::from)
        .unwrap();

    let create_request = InstallContractRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_gas_limit(1_000_000)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(VM2_CEP18.clone())
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now())
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &mut global_state,
        state_root_hash,
        create_request,
    );

    state_root_hash = global_state
        .commit(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_callee_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(1000)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(VM2_CEP18_CALLER))
        .with_serialized_input((create_result.contract_hash().value(),))
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now())
        .build()
        .expect("should build");

    let _effects_2 = run_wasm_session(
        &mut executor,
        &mut global_state,
        state_root_hash,
        execute_request,
    );
}

fn make_global_state_with_genesis() -> (LmdbGlobalState, Digest, TempDir) {
    let default_accounts = vec![GenesisAccount::Account {
        public_key: DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        balance: Motes::new(U512::from(100 * CSPR)),
        validator: None,
    }];

    let (global_state, _state_root_hash, _tempdir) =
        global_state::state::lmdb::make_temporary_global_state([]);

    let genesis_config = GenesisConfigBuilder::default()
        .with_accounts(default_accounts)
        .build();
    let genesis_request: GenesisRequest = GenesisRequest::new(
        Digest::hash("foo"),
        ProtocolVersion::V2_0_0,
        genesis_config,
        ChainspecRegistry::new_with_genesis(b"", b""),
    );
    match global_state.genesis(genesis_request) {
        GenesisResult::Failure(failure) => panic!("Failed to run genesis: {:?}", failure),
        GenesisResult::Fatal(fatal) => panic!("Fatal error while running genesis: {}", fatal),
        GenesisResult::Success {
            post_state_hash,
            effects: _,
        } => (global_state, post_state_hash, _tempdir),
    }
}

#[test]
fn traits() {
    let mut executor = make_executor();
    let (mut global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let execute_request = base_execute_builder()
        .with_target(ExecutionKind::SessionBytes(VM2_TRAIT))
        .with_serialized_input(())
        .with_shared_address_generator(make_address_generator())
        .build()
        .expect("should build");

    run_wasm_session(
        &mut executor,
        &mut global_state,
        state_root_hash,
        execute_request,
    );
}

#[test]
fn upgradable() {
    let mut executor = make_executor();

    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let upgradable_address;

    state_root_hash = {
        let input_data = borsh::to_vec(&(0u8,)).map(Bytes::from).unwrap();

        let create_request = base_install_request_builder()
            .with_wasm_bytes(VM2_UPGRADABLE.clone())
            .with_shared_address_generator(Arc::clone(&address_generator))
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_entry_point("new".to_string())
            .with_input(input_data)
            .build()
            .expect("should build");

        let create_result = run_create_contract(
            &mut executor,
            &mut global_state,
            state_root_hash,
            create_request,
        );

        upgradable_address = *create_result.contract_hash();

        global_state
            .commit(state_root_hash, create_result.effects().clone())
            .expect("Should commit")
    };

    let version_before_upgrade = {
        let address = EntityAddr::new_smart_contract(upgradable_address.value());
        let execute_request = base_execute_builder()
            .with_target(ExecutionKind::Stored {
                address,
                entry_point: "version".to_string(),
            })
            .with_input(Bytes::new())
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_shared_address_generator(Arc::clone(&address_generator))
            .build()
            .expect("should build");
        let res = run_wasm_session(
            &mut executor,
            &mut global_state,
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
        let address = EntityAddr::new_smart_contract(upgradable_address.value());
        let execute_request = base_execute_builder()
            .with_target(ExecutionKind::Stored {
                address,
                entry_point: "increment".to_string(),
            })
            .with_input(Bytes::new())
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_shared_address_generator(Arc::clone(&address_generator))
            .build()
            .expect("should build");
        let res = run_wasm_session(
            &mut executor,
            &mut global_state,
            state_root_hash,
            execute_request,
        );
        state_root_hash = global_state
            .commit(state_root_hash, res.effects().clone())
            .expect("Should commit");
    };

    let binding = VM2_UPGRADABLE_V2;
    let new_code = binding.as_ref();

    let address = EntityAddr::new_smart_contract(upgradable_address.value());
    let execute_request = base_execute_builder()
        .with_transferred_value(0)
        .with_target(ExecutionKind::Stored {
            address,
            entry_point: "perform_upgrade".to_string(),
        })
        .with_gas_limit(DEFAULT_GAS_LIMIT * 10)
        .with_serialized_input((new_code,))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .build()
        .expect("should build");
    let res = run_wasm_session(
        &mut executor,
        &mut global_state,
        state_root_hash,
        execute_request,
    );
    state_root_hash = global_state
        .commit(state_root_hash, res.effects().clone())
        .expect("Should commit");

    let version_after_upgrade = {
        let address = EntityAddr::new_smart_contract(upgradable_address.value());
        let execute_request = base_execute_builder()
            .with_target(ExecutionKind::Stored {
                address,
                entry_point: "version".to_string(),
            })
            .with_input(Bytes::new())
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_shared_address_generator(Arc::clone(&address_generator))
            .build()
            .expect("should build");
        let res = run_wasm_session(
            &mut executor,
            &mut global_state,
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
        let address = EntityAddr::new_smart_contract(upgradable_address.value());
        let execute_request = base_execute_builder()
            .with_target(ExecutionKind::Stored {
                address,
                entry_point: "increment_by".to_string(),
            })
            .with_serialized_input((10u64,))
            .with_gas_limit(DEFAULT_GAS_LIMIT)
            .with_transferred_value(0)
            .with_shared_address_generator(Arc::clone(&address_generator))
            .build()
            .expect("should build");
        let res = run_wasm_session(
            &mut executor,
            &mut global_state,
            state_root_hash,
            execute_request,
        );
        state_root_hash = global_state
            .commit(state_root_hash, res.effects().clone())
            .expect("Should commit");
    };

    let _ = state_root_hash;
}

fn run_create_contract(
    executor: &mut ExecutorV2,
    global_state: &LmdbGlobalState,
    pre_state_hash: Digest,
    install_contract_request: InstallContractRequest,
) -> InstallContractResult {
    executor
        .install_contract(pre_state_hash, global_state, install_contract_request)
        .expect("Succeed")
}

fn run_wasm_session(
    executor: &mut ExecutorV2,
    global_state: &LmdbGlobalState,
    pre_state_hash: Digest,
    execute_request: ExecuteRequest,
) -> ExecuteWithProviderResult {
    let result = executor
        .execute_with_provider(pre_state_hash, global_state, execute_request)
        .expect("Succeed");

    if let Some(host_error) = result.host_error {
        panic!("Host error: {host_error:?}")
    }

    result
}

#[test]
fn backwards_compatibility() {
    let (mut global_state, post_state_hash, _temp) = {
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
    // Calling legacy contract directly by it's address
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

    let mut executor = make_executor();
    let address_generator = make_address_generator();

    let runtime_args = RuntimeArgs::new().to_bytes().unwrap();

    let execute_request = base_execute_builder()
        .with_target(ExecutionKind::Stored {
            address: EntityAddr::new_smart_contract(*counter_hash),
            entry_point: "counter_get".to_string(),
        })
        .with_input(runtime_args.into())
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .build()
        .expect("should build");
    let res = run_wasm_session(
        &mut executor,
        &mut global_state,
        state_root_hash,
        execute_request,
    );
    state_root_hash = global_state
        .commit(state_root_hash, res.effects().clone())
        .expect("Should commit");

    //
    // Instantiate v2 runtime proxy contract
    //
    let input_data = counter_hash.to_vec();
    let install_request = base_install_request_builder()
        .with_wasm_bytes(VM2_LEGACY_COUNTER_PROXY.clone())
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data.into())
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &mut global_state,
        state_root_hash,
        install_request,
    );

    state_root_hash = create_result.post_state_hash();

    let proxy_address = create_result.contract_hash().value();

    // Call v2 contract

    let call_request = base_execute_builder()
        .with_target(ExecutionKind::Stored {
            address: EntityAddr::new_smart_contract(proxy_address),
            entry_point: "perform_test".to_string(),
        })
        .with_input(Bytes::new())
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .build()
        .expect("should build");

    run_wasm_session(
        &mut executor,
        &mut global_state,
        state_root_hash,
        call_request,
    );
}
