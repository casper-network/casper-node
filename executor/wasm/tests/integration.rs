mod chainspec_config;
mod genesis_config_builder;

use std::{
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use casper_execution_engine::engine_state::{EngineConfig, ExecutionEngineV1};
use casper_executor_wasm::{
    install::{
        InstallContractError, InstallContractRequest, InstallContractRequestBuilder,
        InstallContractResult,
    },
    ExecutorConfigBuilder, ExecutorKind, ExecutorV2,
};
use casper_executor_wasm_common::error::CallError;
use casper_executor_wasm_interface::executor::{
    ExecuteError, ExecuteRequest, ExecuteRequestBuilder, ExecuteWithProviderError,
    ExecuteWithProviderResult, ExecutionKind,
};
use casper_storage::{
    data_access_layer::{
        prefixed_values::{PrefixedValuesRequest, PrefixedValuesResult},
        GenesisRequest, GenesisResult, MessageTopicsRequest, MessageTopicsResult, QueryRequest,
        QueryResult,
    },
    global_state::{
        self,
        state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
        transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
    system::runtime_native::{Config, Id, TransferConfig},
    AddressGenerator, KeyPrefix, RuntimeNativeConfig,
};
use casper_types::{
    account::AccountHash, bytesrepr::ToBytes, testing::TestRng, BlockHash, Chainspec,
    ChainspecRegistry, Digest, EntityAddr, FeeHandling, GenesisAccount, GenesisConfig,
    HoldBalanceHandling, HostFunctionCostsV2, HostFunctionV2, Key, MessageLimits, Motes, Phase,
    ProtocolVersion, PublicKey, RefundHandling, SecretKey, StorageCosts, StoredValue, SystemConfig,
    Timestamp, TransactionHash, TransactionV1Hash, WasmConfig, WasmV2Config, U512,
};
use fs_extra::dir;
use itertools::Itertools;
use num_rational::Ratio;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use tempfile::TempDir;

use crate::chainspec_config::ChainspecConfig;
pub(crate) use genesis_config_builder::GenesisConfigBuilder;

/// Default number of validator slots.
pub const DEFAULT_VALIDATOR_SLOTS: u32 = 5;
/// Default auction delay.
pub const DEFAULT_AUCTION_DELAY: u64 = 1;
/// Default lock-in period is currently zero.
pub const DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 0;
/// Default length of total vesting schedule is currently zero.
pub const DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS: u64 = 0;

/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u64 = 7;

/// Round seigniorage rate represented as a fraction of the total supply.
///
/// Annual issuance: 8%
/// Minimum round length: 2^14 ms
/// Ticks per year: 31536000000
///
/// (1+0.08)^((2^14)/31536000000)-1 is expressed as a fractional number below.
pub const DEFAULT_ROUND_SEIGNIORAGE_RATE: Ratio<u64> = Ratio::new_raw(1, 4200000000000000000);
/// Default genesis timestamp in milliseconds.
pub const DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64 = 0;
/// Default block time.
pub const DEFAULT_BLOCK_TIME: u64 = 0;
/// Default gas price.
pub const DEFAULT_GAS_PRICE: u8 = 1;
/// Amount named argument.
pub const ARG_AMOUNT: &str = "amount";
/// Timestamp increment in milliseconds.
pub const TIMESTAMP_MILLIS_INCREMENT: u64 = 30_000; // 30 seconds
/// Default gas hold balance handling.
pub const DEFAULT_GAS_HOLD_BALANCE_HANDLING: HoldBalanceHandling = HoldBalanceHandling::Accrued;
/// Default gas hold interval in milliseconds.
pub const DEFAULT_GAS_HOLD_INTERVAL_MILLIS: u64 = 24 * 60 * 60 * 60;

/// Default value for maximum associated keys configuration option.
pub const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;

/// Default value for a maximum query depth configuration option.
pub const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
/// Default value for maximum runtime call stack height configuration option.
pub const DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32 = 12;
/// Default value for minimum delegation amount in motes.
pub const DEFAULT_MINIMUM_DELEGATION_AMOUNT: u64 = 500 * 1_000_000_000;
/// Default value for maximum delegation amount in motes.
pub const DEFAULT_MAXIMUM_DELEGATION_AMOUNT: u64 = 1_000_000_000 * 1_000_000_000;

/// Default genesis config hash.
pub const DEFAULT_GENESIS_CONFIG_HASH: Digest = Digest::from_raw([42; 32]);

/// Default test account address.
pub static DEFAULT_ACCOUNT_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_ACCOUNT_PUBLIC_KEY));
// NOTE: declaring DEFAULT_ACCOUNT_KEY as *DEFAULT_ACCOUNT_ADDR causes tests to stall.
/// Default account key.
pub static DEFAULT_ACCOUNT_KEY: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_ACCOUNT_PUBLIC_KEY));
/// Default initial balance of a test account in motes.
pub const DEFAULT_ACCOUNT_INITIAL_BALANCE: u64 = 10_000_000_000_000_000_000_u64;
/// Minimal amount for a transfer that creates new accounts.
pub const MINIMUM_ACCOUNT_CREATION_BALANCE: u64 = 7_500_000_000_000_000_u64;
/// Default proposer public key.
pub static DEFAULT_PROPOSER_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([198; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
/// Default proposer address.
pub static DEFAULT_PROPOSER_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*DEFAULT_PROPOSER_PUBLIC_KEY));
/// Default accounts.
pub static DEFAULT_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut ret = Vec::new();
    let genesis_account = GenesisAccount::account(
        DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        None,
    );
    ret.push(genesis_account);
    let proposer_account = GenesisAccount::account(
        DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
        Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        None,
    );
    ret.push(proposer_account);
    let rng = &mut TestRng::new();
    for _ in 0..10 {
        let filler_account = GenesisAccount::account(
            PublicKey::random(rng),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            None,
        );
        ret.push(filler_account);
    }
    ret
});
/// Default [`ProtocolVersion`].
pub const DEFAULT_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V2_0_0;
/// Default [`ChainspecRegistry`].
pub static DEFAULT_CHAINSPEC_REGISTRY: Lazy<ChainspecRegistry> =
    Lazy::new(|| ChainspecRegistry::new_with_genesis(&[1, 2, 3], &[4, 5, 6]));

static DEFAULT_ACCOUNT_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap());
static DEFAULT_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*DEFAULT_ACCOUNT_SECRET_KEY));
static DEFAULT_ACCOUNT_HASH: Lazy<AccountHash> =
    Lazy::new(|| DEFAULT_ACCOUNT_PUBLIC_KEY.to_account_hash());

const CSPR: u64 = 10u64.pow(9);

static RUST_WORKSPACE_PATH: Lazy<PathBuf> = Lazy::new(|| {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR should have parent");
    assert!(
        path.exists(),
        "Workspace path {} does not exists",
        path.display()
    );
    path.to_path_buf()
});

static RUST_WORKSPACE_WASM_PATH: Lazy<PathBuf> = Lazy::new(|| {
    let path = RUST_WORKSPACE_PATH
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release");
    assert!(
        path.exists() || RUST_TOOL_WASM_PATH.exists(),
        "Rust Wasm path {} does not exists",
        path.display()
    );
    path
});
// The location of compiled Wasm files if running from within the 'tests' crate generated by the
// cargo_casper tool, i.e. 'wasm/'.
static RUST_TOOL_WASM_PATH: Lazy<PathBuf> = Lazy::new(|| {
    env::current_dir()
        .expect("should get current working dir")
        .join("wasm")
});
const CHAINSPEC_NAME: &str = "chainspec.toml";

/// Symlink to chainspec.
pub static CHAINSPEC_SYMLINK: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/")
        .join(chainspec_config::CHAINSPEC_NAME)
});

#[track_caller]
fn read_wasm<P: AsRef<Path>>(filename: P) -> Bytes {
    let paths = vec![
        RUST_WORKSPACE_WASM_PATH.clone(),
        RUST_TOOL_WASM_PATH.clone(),
    ];

    for path in &paths {
        let wasm_path = path.join(&filename);
        match fs::read(wasm_path) {
            Ok(bytes) => return Bytes::from(bytes),
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    continue;
                } else {
                    panic!(
                        "Failed to read Wasm file at {}: {}",
                        filename.as_ref().display(),
                        err
                    );
                }
            }
        }
    }

    panic!(
        "Failed to find Wasm file at {} in any of the paths: {:?}",
        filename.as_ref().display(),
        paths
    );
}

const TRANSACTION_HASH_BYTES: [u8; 32] = [55; 32];
const TRANSACTION_HASH: TransactionHash =
    TransactionHash::V1(TransactionV1Hash::from_raw(TRANSACTION_HASH_BYTES));
const DEFAULT_GAS_LIMIT: u64 = 1_000_000 * CSPR;
const DEFAULT_CHAIN_NAME: &str = "casper-example";

// TODO: This is a temporary value, it should be set in the config. Default value from V1 engine
// does not apply to V2 engine due to different cost structure. Rather than hardcoding it here, we
// should probably reflect gas costs in a dynamic costs in host function charge. Proper value is
// pending calculation.
const DEFAULT_GAS_PER_BYTE_COST: u32 = 1_117_587;

fn make_address_generator() -> Arc<RwLock<AddressGenerator>> {
    let id = Id::Transaction(TRANSACTION_HASH);
    Arc::new(RwLock::new(AddressGenerator::new(
        &id.seed(),
        Phase::Session,
    )))
}

fn make_runtime_config(chainspec_config: &ChainspecConfig) -> RuntimeNativeConfig {
    let protocol_version = ProtocolVersion::V2_0_0;
    let transfer_config = TransferConfig::Unadministered;
    let fee_handling = chainspec_config.core_config.fee_handling;
    let refund_handling = chainspec_config.core_config.refund_handling;
    let vesting_schedule_period_millis = chainspec_config
        .core_config
        .vesting_schedule_period
        .millis();
    let allow_auction_bids = chainspec_config.core_config.allow_auction_bids;
    let compute_rewards = chainspec_config.core_config.compute_rewards;
    let max_delegators_per_validator = chainspec_config.core_config.max_delegators_per_validator;
    let minimum_bid_amount = chainspec_config.core_config.minimum_bid_amount;
    let minimum_delegation_amount = chainspec_config.core_config.minimum_delegation_amount;
    let balance_hold_interval = chainspec_config.core_config.gas_hold_interval.millis();
    let include_credits = chainspec_config.core_config.fee_handling == FeeHandling::NoFee;
    let credit_cap = Ratio::new_raw(
        U512::from(*chainspec_config.core_config.validator_credit_cap.numer()),
        U512::from(*chainspec_config.core_config.validator_credit_cap.denom()),
    );
    let enable_addressable_entity = chainspec_config.core_config.enable_addressable_entity;
    let native_transfer_cost = chainspec_config.system_costs_config.mint_costs().transfer;
    Config::new(
        protocol_version,
        transfer_config,
        fee_handling,
        refund_handling,
        vesting_schedule_period_millis,
        allow_auction_bids,
        compute_rewards,
        max_delegators_per_validator,
        minimum_bid_amount,
        minimum_delegation_amount,
        balance_hold_interval,
        include_credits,
        credit_cap,
        enable_addressable_entity,
        native_transfer_cost,
    )
}

fn base_execute_builder(chainspec_config: &ChainspecConfig) -> ExecuteRequestBuilder {
    ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(1000)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(Digest::hash(b"state"))
        .with_block_height(1)
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
}

fn base_install_request_builder(
    chainspec_config: &ChainspecConfig,
) -> InstallContractRequestBuilder {
    InstallContractRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(Digest::hash(b"state"))
        .with_block_height(1)
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"block1")))
}

pub(crate) fn make_executor(chainspec_config: &ChainspecConfig) -> ExecutorV2 {
    let storage_costs = chainspec_config.storage_costs;
    let v1_config = EngineConfig::from(chainspec_config.clone());
    let execution_engine_v1 = ExecutionEngineV1::new(v1_config);
    let wasm_v2_config = chainspec_config.wasm_config.v2().clone();
    let memory_limit = wasm_v2_config.max_memory();
    let message_limits = chainspec_config.wasm_config.messages_limits();
    let executor_config = ExecutorConfigBuilder::default()
        .with_memory_limit(memory_limit)
        .with_executor_kind(ExecutorKind::Compiled)
        .with_wasm_config(wasm_v2_config)
        .with_storage_costs(storage_costs)
        .with_message_limits(message_limits)
        .build()
        .expect("Should build");
    ExecutorV2::new(executor_config, Arc::new(execution_engine_v1))
}

#[test]
fn harness() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

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
            &mut global_state,
            state_root_hash,
            install_request,
        );

        flipper_address = *create_result.smart_contract_addr();

        global_state
            .commit_effects(state_root_hash, create_result.effects().clone())
            .expect("Should commit")
    };

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(1000)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(read_wasm("vm2-harness.wasm")))
        .with_serialized_input((flipper_address,))
        .with_shared_address_generator(address_generator)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(state_root_hash)
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::hash(b"bl0ck")))
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
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
fn cep18() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

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
        &mut global_state,
        state_root_hash,
        create_request,
    );

    dbg!(create_result.gas_usage().gas_spent());

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

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_caller_key(Key::Account(*DEFAULT_ACCOUNT_HASH))
        .with_gas_limit(DEFAULT_GAS_LIMIT)
        .with_transferred_value(1000)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_target(ExecutionKind::SessionBytes(read_wasm(
            "vm2_cep18_caller.wasm",
        )))
        .with_serialized_input((create_result.smart_contract_addr(),))
        .with_transferred_value(0)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(block_time_2)
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(2) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .with_runtime_native_config(make_runtime_config(&chainspec_config))
        .build()
        .expect("should build");

    let result_2 = run_wasm_session(
        &mut executor,
        &mut global_state,
        state_root_hash,
        execute_request,
    );
    dbg!(result_2.gas_usage().gas_spent());

    state_root_hash = global_state
        .commit_effects(state_root_hash, result_2.effects().clone())
        .expect("Should commit");

    let MessageTopicsResult::Success { message_topics } =
        global_state.message_topics(MessageTopicsRequest::new(state_root_hash, contract_hash))
    else {
        panic!("Expected success")
    };

    assert!(matches!(message_topics.get("Transfer"), Some(_)));
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

fn make_global_state_with_genesis() -> (LmdbGlobalState, Digest, TempDir) {
    let default_accounts = vec![GenesisAccount::Account {
        public_key: DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        balance: Motes::new(U512::from(100 * CSPR)),
        validator: None,
    }];

    let (global_state, _state_root_hash, _tempdir) =
        global_state::state::lmdb::make_temporary_global_state([]);

    let genesis_config = GenesisConfig::new(
        default_accounts,
        WasmConfig::default(),
        SystemConfig::default(),
        10,
        10,
        0,
        Default::default(),
        14,
        Timestamp::now().millis(),
        casper_types::HoldBalanceHandling::Accrued,
        0,
        true,
        StorageCosts::default(),
    );
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
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);
    let (mut global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let execute_request = base_execute_builder(&chainspec_config)
        .with_target(ExecutionKind::SessionBytes(read_wasm("vm2_trait.wasm")))
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
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

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
            &mut global_state,
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
        let res = run_wasm_session(
            &mut executor,
            &mut global_state,
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
        let execute_request = base_execute_builder(&chainspec_config)
            .with_target(ExecutionKind::Stored {
                address: upgradable_address,
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
            .commit_effects(state_root_hash, res.effects().clone())
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
        .with_wasm_bytes(read_wasm("vm2_legacy_counter_proxy.wasm"))
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
        &mut global_state,
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

    run_wasm_session(
        &mut executor,
        &mut global_state,
        state_root_hash,
        call_request,
    );
}

// host function tests

fn call_dummy_host_fn_by_name(
    host_function_name: &str,
    gas_limit: u64,
) -> Result<InstallContractResult, InstallContractError> {
    let executor = {
        let execution_engine_v1 = ExecutionEngineV1::default();
        let default_wasm_config = WasmV2Config::default();
        let wasm_config = WasmV2Config::new(
            default_wasm_config.max_memory(),
            default_wasm_config.opcode_costs(),
            HostFunctionCostsV2 {
                read: HostFunctionV2::fixed(1),
                write: HostFunctionV2::fixed(1),
                remove: HostFunctionV2::fixed(1),
                copy_input: HostFunctionV2::fixed(1),
                ret: HostFunctionV2::fixed(1),
                create: HostFunctionV2::fixed(1),
                transfer: HostFunctionV2::fixed(1),
                env_balance: HostFunctionV2::fixed(1),
                upgrade: HostFunctionV2::fixed(1),
                call: HostFunctionV2::fixed(1),
                print: HostFunctionV2::fixed(1),
                emit: HostFunctionV2::fixed(1),
                env_info: HostFunctionV2::fixed(1),
            },
        );
        let executor_config = ExecutorConfigBuilder::default()
            .with_memory_limit(17)
            .with_executor_kind(ExecutorKind::Compiled)
            .with_wasm_config(wasm_config)
            .with_storage_costs(StorageCosts::default())
            .with_message_limits(MessageLimits::default())
            .build()
            .expect("Should build");
        ExecutorV2::new(executor_config, Arc::new(execution_engine_v1))
    };

    let (mut global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let input_data = borsh::to_vec(&(host_function_name.to_owned(),))
        .map(Bytes::from)
        .unwrap();

    let create_request = InstallContractRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_gas_limit(gas_limit)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(read_wasm("vm2_host.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .with_input(input_data)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
        .build()
        .expect("should build");

    executor.install_contract(state_root_hash, &mut global_state, create_request)
}

fn assert_consumes_gas(host_function_name: &str) {
    let result = call_dummy_host_fn_by_name(host_function_name, 1);
    assert!(result.is_err_and(|e| match e {
        InstallContractError::Constructor {
            host_error: CallError::CalleeGasDepleted,
        } => true,
        _ => false,
    }));
}

#[test]
fn host_functions_consume_gas() {
    assert_consumes_gas("get_caller");
    assert_consumes_gas("get_block_time");
    assert_consumes_gas("get_transferred_value");
    assert_consumes_gas("get_balance_of");
    assert_consumes_gas("call");
    assert_consumes_gas("input");
    assert_consumes_gas("create");
    assert_consumes_gas("print");
    assert_consumes_gas("read");
    assert_consumes_gas("ret");
    assert_consumes_gas("transfer");
    assert_consumes_gas("upgrade");
    assert_consumes_gas("write");
}

#[allow(dead_code)]
fn write_n_bytes_at_limit(
    bytes_len: u64,
    gas_limit: u64,
) -> Result<InstallContractResult, InstallContractError> {
    let executor = {
        let execution_engine_v1 = ExecutionEngineV1::default();
        let default_wasm_config = WasmV2Config::default();
        let wasm_config = WasmV2Config::new(
            default_wasm_config.max_memory(),
            default_wasm_config.opcode_costs(),
            HostFunctionCostsV2 {
                read: HostFunctionV2::fixed(0),
                write: HostFunctionV2::fixed(0),
                remove: HostFunctionV2::fixed(0),
                copy_input: HostFunctionV2::fixed(0),
                ret: HostFunctionV2::fixed(0),
                create: HostFunctionV2::fixed(0),
                transfer: HostFunctionV2::fixed(0),
                env_balance: HostFunctionV2::fixed(0),
                upgrade: HostFunctionV2::fixed(0),
                call: HostFunctionV2::fixed(0),
                print: HostFunctionV2::fixed(0),
                emit: HostFunctionV2::fixed(0),
                env_info: HostFunctionV2::fixed(0),
            },
        );
        let executor_config = ExecutorConfigBuilder::default()
            .with_memory_limit(17)
            .with_executor_kind(ExecutorKind::Compiled)
            .with_wasm_config(wasm_config)
            .with_storage_costs(StorageCosts::new(1))
            .with_message_limits(MessageLimits::default())
            .build()
            .expect("Should build");
        ExecutorV2::new(executor_config, Arc::new(execution_engine_v1))
    };

    let (mut global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let input_data = borsh::to_vec(&(bytes_len,)).map(Bytes::from).unwrap();

    let create_request = InstallContractRequestBuilder::default()
        .with_initiator(*DEFAULT_ACCOUNT_HASH)
        .with_gas_limit(gas_limit)
        .with_transaction_hash(TRANSACTION_HASH)
        .with_wasm_bytes(read_wasm("vm2_host.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new_with_write".to_string())
        .with_input(input_data)
        .with_chain_name(DEFAULT_CHAIN_NAME)
        .with_block_time(Timestamp::now().into())
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
        .build()
        .expect("should build");

    executor.install_contract(state_root_hash, &mut global_state, create_request)
}

// #[test]
// fn consume_gas_on_write() {
//     let successful_write = write_n_bytes_at_limit(50, 10_000);
//     assert!(successful_write.is_ok());

//     let out_of_gas_write_exceeded_gas_limit = write_n_bytes_at_limit(50, 10);
//     assert!(out_of_gas_write_exceeded_gas_limit.is_err_and(|e| match e {
//         InstallContractError::Constructor {
//             host_error: HostError::CalleeGasDepleted,
//         } => true,
//         _ => false,
//     }));
// }

#[test]
fn non_existing_smart_contract_does_not_panic() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let address_generator = make_address_generator();
    let executor = make_executor(&chainspec_config);
    let (mut global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

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
        .execute_with_provider(state_root_hash, &mut global_state, execute_request)
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
    let (mut global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

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
        &mut global_state,
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

    let execute_result = run_wasm_session(
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
        casper_types::execution::TransformKindV2::Ret(bytes) => {
            // The ret function in the test contract calls casper::ret with [1, 2, 3] data
            assert_eq!(
                &bytes.to_bytes().expect("must get to bytes"),
                &[1, 2, 3],
                "Return data should match what was passed to casper::ret"
            );
        }
        _ => panic!("Expected Ret transform kind"),
    }

    // Verify the key is the contract address
    let expected_key = casper_types::Key::SmartContract(contract_address);
    assert_eq!(
        ret_transform.key(),
        &expected_key,
        "Ret transform should be under the contract key"
    );
}
