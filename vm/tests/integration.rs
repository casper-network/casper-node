use std::sync::Arc;

use bytes::Bytes;
use casper_storage::{
    address_generator::Address,
    data_access_layer::{GenesisRequest, GenesisResult},
    global_state::{
        self,
        state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
    },
    system::runtime_native::Id,
    AddressGenerator,
};
use casper_types::{
    account::AccountHash,
    execution::{Effects, TransformKindV2},
    ChainspecRegistry, Digest, EntityAddr, GenesisAccount, GenesisConfigBuilder, Key, Motes, Phase,
    ProtocolVersion, PublicKey, SecretKey, TransactionHash, TransactionV1Hash, U512,
};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use tempfile::TempDir;
use vm::executor::{
    v2::{ExecutorConfigBuilder, ExecutorKind, ExecutorV2},
    ExecuteRequest, ExecuteRequestBuilder, ExecutionKind, Executor,
};

static DEFAULT_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap());
static DEFAULT_ACCOUNT_PUBLIC_KEY: Lazy<casper_types::PublicKey> =
    Lazy::new(|| PublicKey::from(&*DEFAULT_ACCOUNT_SECRET_KEY));
static DEFAULT_ACCOUNT_HASH: Lazy<AccountHash> =
    Lazy::new(|| DEFAULT_ACCOUNT_PUBLIC_KEY.to_account_hash());

const CSPR: u64 = 10u64.pow(9);

const VM2_TEST_CONTRACT: Bytes = Bytes::from_static(include_bytes!("../vm2-test-contract.wasm"));
const VM2_HARNESS: Bytes = Bytes::from_static(include_bytes!("../vm2-harness.wasm"));
const VM2_CEP18: Bytes = Bytes::from_static(include_bytes!("../vm2-cep18.wasm"));
const VM2_CEP18_CALLER: Bytes = Bytes::from_static(include_bytes!("../vm2-cep18-caller.wasm"));
const VM2_TRAITS: Bytes = Bytes::from_static(include_bytes!("../vm2-trait.wasm"));

const TRANSACTION_HASH_BYTES: [u8; 32] = [55; 32];
const TRANSACTION_HASH: TransactionHash =
    TransactionHash::V1(TransactionV1Hash::from_raw(TRANSACTION_HASH_BYTES));
fn make_address_generator() -> Arc<RwLock<AddressGenerator>> {
    let mut rng = rand::thread_rng();
    let id = Id::Transaction(TRANSACTION_HASH);
    let address_generator = Arc::new(RwLock::new(AddressGenerator::new(
        &id.seed(),
        Phase::Session,
    )));
    Arc::new(RwLock::new(AddressGenerator::new(
        &id.seed(),
        Phase::Session,
    )))
}

fn base_execute_builder() -> ExecuteRequestBuilder {
    ExecuteRequestBuilder::default()
        .with_caller(DEFAULT_ACCOUNT_HASH.value())
        .with_gas_limit(1_000_000)
        .with_value(1000)
        .with_transaction_hash(TRANSACTION_HASH)
}

#[test]
fn test_contract() {
    let mut executor = make_executor();

    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let input = ("Hello, world!".to_string(), 123456789u32);
    let execute_request = base_execute_builder()
        .with_target(ExecutionKind::WasmBytes(VM2_TEST_CONTRACT))
        .with_serialized_input(input)
        .build()
        .expect("should build");

    let _effects = run_wasm(
        &mut executor,
        &mut global_state,
        make_address_generator(),
        state_root_hash,
        execute_request,
    );
}

#[test]
fn harness() {
    let mut executor = make_executor();

    let (mut global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let execute_request = base_execute_builder()
        .with_target(ExecutionKind::WasmBytes(VM2_HARNESS))
        .with_serialized_input(())
        .build()
        .expect("should build");

    run_wasm(
        &mut executor,
        &mut global_state,
        make_address_generator(),
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

    let execute_request = base_execute_builder()
        .with_target(ExecutionKind::WasmBytes(VM2_CEP18))
        .with_serialized_input(())
        .with_value(0)
        .build()
        .expect("should build");

    let effects_1 = run_wasm(
        &mut executor,
        &mut global_state,
        make_address_generator(),
        state_root_hash,
        execute_request,
    );

    let contract_hash = {
        let mut values: Vec<_> = effects_1
            .transforms()
            .iter()
            .filter(|t| t.key().is_smart_contract_key() && t.kind() != &TransformKindV2::Identity)
            .collect();
        assert_eq!(values.len(), 1, "{values:#?}");
        let transform = values.remove(0);
        let Key::AddressableEntity(EntityAddr::SmartContract(contract_hash)) = transform.key()
        else {
            panic!("Expected a smart contract key")
        };
        *contract_hash
    };

    state_root_hash = global_state
        .commit(state_root_hash, effects_1)
        .expect("Should commit");

    let execute_request = base_execute_builder()
        .with_target(ExecutionKind::WasmBytes(VM2_CEP18_CALLER))
        .with_serialized_input((contract_hash,))
        .with_value(0)
        .build()
        .expect("should build");

    let _effects_2 = run_wasm(
        &mut executor,
        &mut global_state,
        make_address_generator(),
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

    let (mut global_state, mut state_root_hash, _tempdir) =
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
        } => {
            state_root_hash = post_state_hash;
        }
    }
    (global_state, state_root_hash, _tempdir)
}

#[test]
fn traits() {
    let mut executor = make_executor();
    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let execute_request = base_execute_builder()
        .with_target(ExecutionKind::WasmBytes(VM2_TRAITS))
        .with_serialized_input(())
        .build()
        .expect("should build");

    run_wasm(
        &mut executor,
        &mut global_state,
        make_address_generator(),
        state_root_hash,
        execute_request,
    );
}

fn run_wasm(
    executor: &mut ExecutorV2,
    global_state: &mut LmdbGlobalState,
    address_generator: Arc<RwLock<AddressGenerator>>,
    pre_state_hash: Digest,
    execute_request: ExecuteRequest,
) -> Effects {
    let tracking_copy = global_state
        .tracking_copy(pre_state_hash)
        .expect("Obtaining root hash succeed")
        .expect("Root hash exists");

    let result = executor
        .execute(tracking_copy, address_generator, execute_request)
        .expect("Succeed");

    result.effects().clone()
}
