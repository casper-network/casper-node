use std::{collections::BTreeMap, iter, path::PathBuf, sync::Arc, time::Duration};

use casper_storage::data_access_layer::{QueryRequest, QueryResult};
use derive_more::{Display, From};
use fs_extra::dir;
use prometheus::Registry;
use rand::RngCore;
use serde::Serialize;
use tempfile::TempDir;

use casper_types::{
    bytesrepr::Bytes, contracts::ProtocolVersionMajor, runtime_args, BlockHash, Chainspec,
    ChainspecRawBytes, Deploy, Digest, EntityVersion, EraId, ExecutableDeployItem, PackageAddr,
    PricingMode, PublicKey, RuntimeArgs, SecretKey, TimeDiff, Timestamp, Transaction,
    TransactionConfig, TransactionRuntimeParams, MINT_LANE_ID, U512,
};

use super::*;
use crate::{
    components::{
        network::Identity as NetworkIdentity,
        storage::{self, Storage},
    },
    effect::announcements::{ContractRuntimeAnnouncement, ControlAnnouncement, FatalAnnouncement},
    protocol::Message,
    reactor::{self, EventQueueHandle, ReactorEvent, Runner},
    testing::{self, network::NetworkedReactor, ConditionCheckReactor},
    types::{
        transaction::{
            calculate_transaction_lane_for_transaction,
            transaction_v1_builder::TransactionV1Builder,
        },
        BlockPayload, ExecutableBlock, FinalizedBlock, InternalEraReport, MetaBlockState,
    },
    utils::{Loadable, WithDir, RESOURCES_PATH},
    NodeRng,
};

const FIXTURES_DIRECTORY: &str = "../execution_engine_testing/tests/fixtures";
fn path_to_lmdb_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES_DIRECTORY)
}

const RECENT_ERA_COUNT: u64 = 5;
const MAX_TTL: TimeDiff = TimeDiff::from_seconds(86400);
const TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize, Display)]
#[must_use]
enum Event {
    #[from]
    ContractRuntime(super::Event),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
    #[from]
    ContractRuntimeAnnouncement(ContractRuntimeAnnouncement),
    #[from]
    Storage(storage::Event),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    MetaBlockAnnouncement(MetaBlockAnnouncement),
}

impl ReactorEvent for Event {
    fn is_control(&self) -> bool {
        false
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        None
    }
}

trait Unhandled {}

impl<T: Unhandled> From<T> for Event {
    fn from(_: T) -> Self {
        unimplemented!("not handled in contract runtime tests")
    }
}

impl Unhandled for ControlAnnouncement {}

impl Unhandled for FatalAnnouncement {}

impl Unhandled for NetworkRequest<Message> {}

impl Unhandled for UnexecutedBlockAnnouncement {}

impl Unhandled for NonExecutableBlockAnnouncement {}

struct TestConfig {
    config: Config,
    fixture_name: Option<String>,
}

struct Reactor {
    storage: Storage,
    contract_runtime: ContractRuntime,
    _storage_tempdir: TempDir,
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = TestConfig;
    type Error = ConfigError;

    fn new(
        config: Self::Config,
        chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        _event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (storage_config, storage_tempdir) = storage::Config::new_for_tests(1);
        if let Some(fixture_name) = config.fixture_name {
            let source = path_to_lmdb_fixtures().join(&fixture_name);
            fs_extra::copy_items(&[source], &storage_tempdir, &dir::CopyOptions::default())
                .expect("should copy global state fixture");
        }

        let storage_withdir = WithDir::new(storage_tempdir.path(), storage_config);
        let storage = Storage::new(
            &storage_withdir,
            None,
            chainspec.protocol_version(),
            EraId::default(),
            "test",
            MAX_TTL.into(),
            RECENT_ERA_COUNT,
            Some(registry),
            false,
            TransactionConfig::default(),
        )
        .unwrap();

        let contract_runtime =
            ContractRuntime::new(storage.root_path(), &config.config, chainspec, registry)?;

        let reactor = Reactor {
            storage,
            contract_runtime,
            _storage_tempdir: storage_tempdir,
        };

        Ok((reactor, Effects::new()))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        trace!(?event);
        match event {
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            Event::ContractRuntimeRequest(req) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            Event::ContractRuntimeAnnouncement(announcement) => {
                info!("{announcement}");
                Effects::new()
            }
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::StorageRequest(req) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            Event::MetaBlockAnnouncement(announcement) => {
                info!("{announcement}");
                Effects::new()
            }
        }
    }
}

impl NetworkedReactor for Reactor {}

/// Schedule the given block and its deploys to be executed by the contract runtime.
fn execute_block(
    executable_block: ExecutableBlock,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder| {
        effect_builder
            .enqueue_block_for_execution(executable_block, MetaBlockState::new())
            .ignore()
    }
}

/// A function to be used a condition check, indicating that execution has started.
fn execution_started(event: &Event) -> bool {
    matches!(
        event,
        Event::ContractRuntimeRequest(ContractRuntimeRequest::EnqueueBlockForExecution { .. })
    )
}

/// A function to be used a condition check, indicating that execution has completed.
fn execution_completed(event: &Event) -> bool {
    matches!(event, Event::MetaBlockAnnouncement(_))
}

#[tokio::test]
async fn should_not_set_shared_pre_state_to_lower_block_height() {
    testing::init_logging();

    let config = Config {
        max_global_state_size: Some(100 * 1024 * 1024),
        ..Config::default()
    };
    let config = TestConfig {
        config,
        fixture_name: None,
    };
    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let chainspec = Arc::new(chainspec);
    let chainspec_raw_bytes = Arc::new(chainspec_raw_bytes);

    let mut rng = crate::new_rng();
    let rng = &mut rng;

    let mut runner: Runner<ConditionCheckReactor<Reactor>> = Runner::new(
        config,
        Arc::clone(&chainspec),
        Arc::clone(&chainspec_raw_bytes),
        rng,
    )
    .await
    .unwrap();

    // Commit genesis to set up initial global state.
    let post_commit_genesis_state_hash = runner
        .reactor()
        .inner()
        .contract_runtime
        .commit_genesis(chainspec.as_ref(), chainspec_raw_bytes.as_ref())
        .as_legacy()
        .unwrap()
        .0;

    let initial_pre_state = ExecutionPreState::new(
        0,
        post_commit_genesis_state_hash,
        BlockHash::default(),
        Digest::default(),
    );
    runner
        .reactor_mut()
        .inner_mut()
        .contract_runtime
        .set_execution_pre_state(initial_pre_state);

    // Create the genesis immediate switch block.
    let block_0 = ExecutableBlock::from_finalized_block_and_transactions(
        FinalizedBlock::new(
            BlockPayload::default(),
            Some(InternalEraReport::default()),
            Timestamp::now(),
            EraId::new(0),
            0,
            PublicKey::System,
        ),
        vec![],
    );

    runner
        .process_injected_effects(execute_block(block_0))
        .await;
    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    // Create the first block of era 1.
    let block_1 = ExecutableBlock::from_finalized_block_and_transactions(
        FinalizedBlock::new(
            BlockPayload::default(),
            None,
            Timestamp::now(),
            EraId::new(1),
            1,
            PublicKey::System,
        ),
        vec![],
    );
    runner
        .process_injected_effects(execute_block(block_1))
        .await;
    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    // Check that the next block height expected by the contract runtime is 2.
    assert_eq!(
        runner
            .reactor()
            .inner()
            .contract_runtime
            .execution_pre_state
            .lock()
            .unwrap()
            .next_block_height(),
        2
    );

    // Prepare to create a block which will take a while to execute, i.e. loaded with many deploys
    // transferring from node-1's main account to new random public keys.
    let node_1_secret_key = SecretKey::from_file(
        RESOURCES_PATH
            .join("local")
            .join("secret_keys")
            .join("node-1.pem"),
    )
    .unwrap();
    let timestamp = Timestamp::now();
    let ttl = TimeDiff::from_seconds(100);
    let gas_price = 1;
    let chain_name = chainspec.network_config.name.clone();
    let payment = ExecutableDeployItem::ModuleBytes {
        module_bytes: Bytes::new(),
        args: runtime_args! {
          "amount" => U512::from(chainspec.system_costs_config.mint_costs().transfer),
        },
    };

    let txns: Vec<Transaction> = iter::repeat_with(|| {
        let target_public_key = PublicKey::random(rng);
        let session = ExecutableDeployItem::Transfer {
            args: runtime_args! {
              "amount" => U512::from(chainspec.transaction_config.native_transfer_minimum_motes),
              "target" => target_public_key,
              "id" => Some(9_u64),
            },
        };
        Transaction::Deploy(Deploy::new_signed(
            timestamp,
            ttl,
            gas_price,
            vec![],
            chain_name.clone(),
            payment.clone(),
            session,
            &node_1_secret_key,
            None,
        ))
    })
    .take(200)
    .collect();

    let mut txn_set = BTreeMap::new();
    let val = txns
        .iter()
        .map(|transaction| {
            let hash = transaction.hash();
            let approvals = transaction.approvals();
            (hash, approvals)
        })
        .collect();
    txn_set.insert(MINT_LANE_ID, val);
    let block_payload = BlockPayload::new(txn_set, vec![], Default::default(), true, 1u8);
    let block_2 = ExecutableBlock::from_finalized_block_and_transactions(
        FinalizedBlock::new(
            block_payload,
            None,
            Timestamp::now(),
            EraId::new(1),
            2,
            PublicKey::System,
        ),
        txns,
    );
    runner
        .process_injected_effects(execute_block(block_2))
        .await;

    // Crank until execution is scheduled.
    runner
        .crank_until(rng, execution_started, TEST_TIMEOUT)
        .await;

    // While executing this block, set the execution pre-state to a later block (as if we had sync
    // leaped and skipped ahead).
    let next_block_height = 9;
    tokio::time::sleep(Duration::from_millis(50)).await;
    runner
        .reactor_mut()
        .inner_mut()
        .contract_runtime
        .set_execution_pre_state(ExecutionPreState::new(
            next_block_height,
            Digest::hash(rng.next_u64().to_le_bytes()),
            BlockHash::random(rng),
            Digest::hash(rng.next_u64().to_le_bytes()),
        ));

    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    let actual = runner
        .reactor()
        .inner()
        .contract_runtime
        .execution_pre_state
        .lock()
        .unwrap()
        .next_block_height();

    let expected = next_block_height;

    // Check that the next block height expected by the contract runtime is `next_block_height` and
    // not 3.
    assert_eq!(actual, expected);
}

fn valid_wasm_txn(
    initiator: &SecretKey,
    chain_name: &str,
    pricing_mode: PricingMode,
    name: &str,
    runtime_args: RuntimeArgs,
) -> Transaction {
    let contract_file = RESOURCES_PATH
        .join("..")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(format!("{name}.wasm"));
    let module_bytes = Bytes::from(std::fs::read(contract_file).expect("cannot read module bytes"));
    let mut txn = Transaction::from(
        TransactionV1Builder::new_session_with_runtime_args(
            true,
            module_bytes,
            TransactionRuntimeParams::VmCasperV1,
            runtime_args,
        )
        .with_chain_name(chain_name)
        .with_pricing_mode(pricing_mode)
        .with_initiator_addr(PublicKey::from(initiator))
        .build()
        .unwrap(),
    );
    txn.sign(initiator);
    txn
}

#[allow(clippy::too_many_arguments)]
fn valid_versioned_call_txn(
    initiator: &SecretKey,
    chain_name: &str,
    pricing_mode: PricingMode,
    entry_point: &str,
    package_hash: PackageAddr,
    runtime_args: RuntimeArgs,
    version: Option<EntityVersion>,
    protocol_version_major: Option<ProtocolVersionMajor>,
) -> Transaction {
    let mut txn = Transaction::from(
        TransactionV1Builder::new_targeting_package_with_runtime_args(
            package_hash,
            version,
            protocol_version_major,
            entry_point,
            TransactionRuntimeParams::VmCasperV1,
            runtime_args,
        )
        .with_chain_name(chain_name)
        .with_pricing_mode(pricing_mode)
        .with_initiator_addr(PublicKey::from(initiator))
        .build()
        .unwrap(),
    );
    txn.sign(initiator);
    txn
}

#[tokio::test]
async fn should_correctly_manage_entity_version_calls() {
    testing::init_logging();

    let config = Config {
        max_global_state_size: Some(100 * 1024 * 1024),
        ..Config::default()
    };
    let config = TestConfig {
        config,
        fixture_name: None,
    };
    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let chainspec = Arc::new(chainspec);
    let chainspec_raw_bytes = Arc::new(chainspec_raw_bytes);

    let mut rng = crate::new_rng();
    let rng = &mut rng;

    let mut runner: Runner<ConditionCheckReactor<Reactor>> = Runner::new(
        config,
        Arc::clone(&chainspec),
        Arc::clone(&chainspec_raw_bytes),
        rng,
    )
    .await
    .unwrap();

    // Commit genesis to set up initial global state.
    let post_commit_genesis_state_hash = runner
        .reactor()
        .inner()
        .contract_runtime
        .commit_genesis(chainspec.as_ref(), chainspec_raw_bytes.as_ref())
        .as_legacy()
        .unwrap()
        .0;

    let initial_pre_state = ExecutionPreState::new(
        0,
        post_commit_genesis_state_hash,
        BlockHash::default(),
        Digest::default(),
    );
    runner
        .reactor_mut()
        .inner_mut()
        .contract_runtime
        .set_execution_pre_state(initial_pre_state);

    // Create the genesis immediate switch block.
    let block_0 = ExecutableBlock::from_finalized_block_and_transactions(
        FinalizedBlock::new(
            BlockPayload::default(),
            Some(InternalEraReport::default()),
            Timestamp::now(),
            EraId::new(0),
            0,
            PublicKey::System,
        ),
        vec![],
    );

    runner
        .process_injected_effects(execute_block(block_0))
        .await;
    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    // Create the first block of era 1.
    let block_1 = ExecutableBlock::from_finalized_block_and_transactions(
        FinalizedBlock::new(
            BlockPayload::default(),
            None,
            Timestamp::now(),
            EraId::new(1),
            1,
            PublicKey::System,
        ),
        vec![],
    );
    runner
        .process_injected_effects(execute_block(block_1))
        .await;
    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    // Prepare to create a block which will take a while to execute, i.e. loaded with many deploys
    // transferring from node-1's main account to new random public keys.
    let node_1_secret_key = SecretKey::from_file(
        RESOURCES_PATH
            .join("local")
            .join("secret_keys")
            .join("node-1.pem"),
    )
    .unwrap();

    let node_1_public_key = PublicKey::from(&node_1_secret_key);
    let chain_name = chainspec.network_config.name.clone();
    let installer_transaction = valid_wasm_txn(
        &node_1_secret_key,
        &chain_name,
        PricingMode::PaymentLimited {
            payment_amount: 250_000_000_000,
            gas_price_tolerance: 3,
            standard_payment: true,
        },
        "purse_holder_stored",
        runtime_args! {
            "is_locked" => false
        },
    );

    let lane_id =
        calculate_transaction_lane_for_transaction(&installer_transaction, &chainspec).unwrap();

    let mut txn_set = BTreeMap::new();
    let txn_hash = installer_transaction.hash();
    let approvals = installer_transaction.approvals();

    txn_set.insert(lane_id, vec![(txn_hash, approvals)]);

    let block_payload = BlockPayload::new(txn_set, vec![], Default::default(), true, 1u8);
    let block_2 = ExecutableBlock::from_finalized_block_and_transactions(
        FinalizedBlock::new(
            block_payload,
            None,
            Timestamp::now(),
            EraId::new(1),
            2,
            PublicKey::System,
        ),
        vec![installer_transaction],
    );
    runner
        .process_injected_effects(execute_block(block_2))
        .await;

    // Crank until execution is scheduled.
    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    let pre_state_hash = {
        let prestate = runner
            .reactor()
            .inner()
            .contract_runtime
            .execution_pre_state
            .lock()
            .expect("must get lock");
        prestate.pre_state_root_hash()
    };

    let key = Key::Account(node_1_public_key.to_account_hash());
    let query_request = QueryRequest::new(pre_state_hash, key, vec![]);

    let package_key = if let QueryResult::Success { value, .. } = runner
        .reactor()
        .inner()
        .contract_runtime
        .data_access_layer
        .query(query_request)
    {
        *value
            .as_account()
            .expect("must get account")
            .named_keys()
            .get("purse_holder")
            .expect("must get package key")
    } else {
        panic!("query failed");
    };

    let package_hash = package_key
        .into_hash_addr()
        .map(PackageAddr::new)
        .expect("must get package hash");

    let upgrader_transaction = valid_wasm_txn(
        &node_1_secret_key,
        &chain_name,
        PricingMode::PaymentLimited {
            payment_amount: 250_000_000_000,
            gas_price_tolerance: 3,
            standard_payment: true,
        },
        "purse_holder_stored_upgrader",
        runtime_args! {
            "contract_package" => package_hash
        },
    );

    let lane_id =
        calculate_transaction_lane_for_transaction(&upgrader_transaction, &chainspec).unwrap();

    let mut txn_set = BTreeMap::new();
    let txn_hash = upgrader_transaction.hash();
    let approvals = upgrader_transaction.approvals();

    txn_set.insert(lane_id, vec![(txn_hash, approvals)]);

    let block_payload = BlockPayload::new(txn_set, vec![], Default::default(), true, 1u8);
    let block_2 = ExecutableBlock::from_finalized_block_and_transactions(
        FinalizedBlock::new(
            block_payload,
            None,
            Timestamp::now(),
            EraId::new(1),
            3,
            PublicKey::System,
        ),
        vec![upgrader_transaction],
    );
    runner
        .process_injected_effects(execute_block(block_2))
        .await;

    // Crank until execution is scheduled.
    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    let pre_state_hash = {
        let prestate = runner
            .reactor()
            .inner()
            .contract_runtime
            .execution_pre_state
            .lock()
            .expect("must get lock");
        prestate.pre_state_root_hash()
    };

    let query_request = QueryRequest::new(pre_state_hash, package_key, vec![]);
    if let QueryResult::Success { value, .. } = runner
        .reactor()
        .inner()
        .contract_runtime
        .data_access_layer
        .query(query_request)
    {
        let versions = value
            .as_contract_package()
            .expect("must get account")
            .versions();

        assert_eq!(2, versions.len())
    } else {
        panic!("query failed");
    };

    let call_by_entity_version_1 = valid_versioned_call_txn(
        &node_1_secret_key,
        &chain_name,
        PricingMode::PaymentLimited {
            payment_amount: 250_000_000_000,
            gas_price_tolerance: 3,
            standard_payment: true,
        },
        "add_named_purse",
        package_hash,
        runtime_args! {
            "purse_name" => "purse"
        },
        Some(1),
        None,
    );

    let call_by_major_version_and_entity_version = valid_versioned_call_txn(
        &node_1_secret_key,
        &chain_name,
        PricingMode::PaymentLimited {
            payment_amount: 250_000_000_000,
            gas_price_tolerance: 3,
            standard_payment: true,
        },
        "add_named_purse",
        package_hash,
        runtime_args! {
            "purse_name" => "purse"
        },
        Some(1),
        Some(2),
    );

    let call_by_major_version = valid_versioned_call_txn(
        &node_1_secret_key,
        &chain_name,
        PricingMode::PaymentLimited {
            payment_amount: 250_000_000_000,
            gas_price_tolerance: 3,
            standard_payment: true,
        },
        "add",
        package_hash,
        runtime_args! {
            "purse_name" => "purse"
        },
        None,
        Some(2),
    );

    let lane_id =
        calculate_transaction_lane_for_transaction(&call_by_entity_version_1, &chainspec).unwrap();

    let txns = vec![
        call_by_entity_version_1,
        call_by_major_version_and_entity_version,
        call_by_major_version,
    ];

    let mut txn_set = BTreeMap::new();
    let val = txns
        .iter()
        .map(|txn| {
            let hash = txn.hash();
            let approvals = txn.approvals();
            (hash, approvals)
        })
        .collect();

    txn_set.insert(lane_id, val);

    let block_payload = BlockPayload::new(txn_set, vec![], Default::default(), true, 1u8);
    let block_3 = ExecutableBlock::from_finalized_block_and_transactions(
        FinalizedBlock::new(
            block_payload,
            None,
            Timestamp::now(),
            EraId::new(1),
            4,
            PublicKey::System,
        ),
        txns.clone(),
    );
    runner
        .process_injected_effects(execute_block(block_3))
        .await;

    // Crank until execution is scheduled.
    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    for txn in txns.iter() {
        let hash = txn.hash();
        let results = runner
            .reactor()
            .inner()
            .storage
            .read_execution_result(&hash)
            .unwrap();

        assert!(results.error_message().is_none())
    }
}

#[cfg(test)]
mod test_mod {
    use std::sync::Arc;

    use prometheus::Registry;
    use rand::Rng;
    use tempfile::tempdir;

    use casper_storage::{
        data_access_layer::{EntryPointExistsRequest, EntryPointExistsResult},
        global_state::{
            state::{CommitProvider, StateProvider},
            trie::Trie,
        },
    };
    use casper_types::{
        account::AccountHash,
        bytesrepr,
        contracts::{ContractPackageHash, EntryPoint, EntryPoints},
        execution::{TransformKindV2, TransformV2},
        global_state::Pointer,
        testing::TestRng,
        ActivationPoint, CLType, CLValue, Chainspec, ChunkWithProof, Contract, ContractWasmHash,
        CoreConfig, Digest, EntityAddr, EntryPointAccess, EntryPointAddr, EntryPointPayment,
        EntryPointType, EntryPointValue, EraId, HashAddr, Key, NamedKeys, ProtocolConfig,
        ProtocolVersion, StoredValue, TimeDiff, DEFAULT_FEE_HANDLING, DEFAULT_GAS_HOLD_INTERVAL,
        DEFAULT_REFUND_HANDLING,
    };

    use super::{Config as ContractRuntimeConfig, ContractRuntime};
    use crate::{
        components::fetcher::FetchResponse,
        contract_runtime::ContractRuntimeError,
        types::{ChunkingError, TrieOrChunk, TrieOrChunkId, ValueOrChunk},
    };

    #[derive(Debug, Clone)]
    struct TestPair(Key, StoredValue);

    fn create_pre_condor_contract(
        rng: &mut TestRng,
        contract_hash: Key,
        entry_point_name: &str,
        protocol_version: ProtocolVersion,
    ) -> Vec<TestPair> {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            entry_point_name,
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Caller,
        );
        entry_points.add_entry_point(entry_point);

        let contract_package_hash = ContractPackageHash::new(rng.gen());
        let contract_wasm_hash = ContractWasmHash::new(rng.gen());
        let named_keys = NamedKeys::new();
        let contract = Contract::new(
            contract_package_hash,
            contract_wasm_hash,
            named_keys,
            entry_points,
            protocol_version,
        );
        vec![TestPair(contract_hash, StoredValue::Contract(contract))]
    }

    fn create_entry_point(entity_addr: EntityAddr, entry_point_name: &str) -> Vec<TestPair> {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            entry_point_name,
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Caller,
        );
        entry_points.add_entry_point(entry_point);
        let key = Key::EntryPoint(
            EntryPointAddr::new_v1_entry_point_addr(entity_addr, entry_point_name).unwrap(),
        );
        let entry_point = casper_types::EntityEntryPoint::new(
            entry_point_name,
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Caller,
            EntryPointPayment::Caller,
        );
        let entry_point_value = EntryPointValue::V1CasperVm(entry_point);
        vec![TestPair(key, StoredValue::EntryPoint(entry_point_value))]
    }

    // Creates the test pairs that contain data of size
    // greater than the chunk limit.
    fn create_test_pairs_with_large_data() -> Vec<TestPair> {
        let val = CLValue::from_t(
            String::from_utf8(vec![b'a'; ChunkWithProof::CHUNK_SIZE_BYTES * 2]).unwrap(),
        )
        .unwrap();
        vec![
            TestPair(
                Key::Account(AccountHash::new([1_u8; 32])),
                StoredValue::CLValue(val.clone()),
            ),
            TestPair(
                Key::Account(AccountHash::new([2_u8; 32])),
                StoredValue::CLValue(val),
            ),
        ]
    }

    fn extract_next_hash_from_trie(trie_or_chunk: TrieOrChunk) -> Digest {
        let next_hash = if let TrieOrChunk::Value(trie_bytes) = trie_or_chunk {
            if let Trie::Node { pointer_block } = bytesrepr::deserialize::<Trie<Key, StoredValue>>(
                trie_bytes.into_inner().into_inner().into(),
            )
            .expect("Could not parse trie bytes")
            {
                if pointer_block.child_count() == 0 {
                    panic!("expected children");
                }
                let (_, ptr) = pointer_block.as_indexed_pointers().next().unwrap();
                match ptr {
                    Pointer::LeafPointer(ptr) | Pointer::NodePointer(ptr) => ptr,
                }
            } else {
                panic!("expected `Node`");
            }
        } else {
            panic!("expected `Trie`");
        };
        next_hash
    }

    // Creates a test ContractRuntime and feeds the underlying GlobalState with `test_pair`.
    // Returns [`ContractRuntime`] instance and the new Merkle root after applying the `test_pair`.
    fn create_test_state(rng: &mut TestRng, test_pair: Vec<TestPair>) -> (ContractRuntime, Digest) {
        let temp_dir = tempdir().unwrap();
        let chainspec = Chainspec {
            protocol_config: ProtocolConfig {
                activation_point: ActivationPoint::EraId(EraId::from(2)),
                ..ProtocolConfig::random(rng)
            },
            core_config: CoreConfig {
                max_associated_keys: 10,
                max_runtime_call_stack_height: 10,
                minimum_delegation_amount: 10,
                prune_batch_size: 5,
                strict_argument_checking: true,
                vesting_schedule_period: TimeDiff::from_millis(1),
                max_delegators_per_validator: 0,
                allow_auction_bids: true,
                allow_unrestricted_transfers: true,
                fee_handling: DEFAULT_FEE_HANDLING,
                refund_handling: DEFAULT_REFUND_HANDLING,
                gas_hold_interval: DEFAULT_GAS_HOLD_INTERVAL,
                ..CoreConfig::random(rng)
            },
            wasm_config: Default::default(),
            system_costs_config: Default::default(),
            ..Chainspec::random(rng)
        };
        let contract_runtime = ContractRuntime::new(
            temp_dir.path(),
            &ContractRuntimeConfig::default(),
            Arc::new(chainspec),
            &Registry::default(),
        )
        .unwrap();
        let empty_state_root = contract_runtime.data_access_layer().empty_root();
        let mut effects = casper_types::execution::Effects::new();
        for TestPair(key, value) in test_pair {
            effects.push(TransformV2::new(key, TransformKindV2::Write(value)));
        }
        let post_state_hash = &contract_runtime
            .data_access_layer()
            .as_ref()
            .commit_effects(empty_state_root, effects)
            .expect("applying effects to succeed");
        (contract_runtime, *post_state_hash)
    }

    fn read_trie(contract_runtime: &ContractRuntime, id: TrieOrChunkId) -> TrieOrChunk {
        let serialized_id = bincode::serialize(&id).unwrap();
        match contract_runtime
            .fetch_trie_local(&serialized_id)
            .expect("expected a successful read")
        {
            FetchResponse::Fetched(found) => found,
            FetchResponse::NotProvided(_) | FetchResponse::NotFound(_) => {
                panic!("expected to find the trie")
            }
        }
    }

    #[test]
    fn fetching_enty_points_falls_back_to_contract() {
        let rng = &mut TestRng::new();
        let hash_addr: HashAddr = rng.gen();
        let contract_hash = Key::Hash(hash_addr);
        let entry_point_name = "ep1";
        let initial_state = create_pre_condor_contract(
            rng,
            contract_hash,
            entry_point_name,
            ProtocolVersion::V2_0_0,
        );
        let (contract_runtime, state_hash) = create_test_state(rng, initial_state);
        let request =
            EntryPointExistsRequest::new(state_hash, entry_point_name.to_string(), hash_addr);
        let res = contract_runtime
            .data_access_layer()
            .entry_point_exists(request);
        assert!(matches!(res, EntryPointExistsResult::Success));
    }

    #[test]
    fn fetching_enty_points_fetches_entry_point_from_v2() {
        let rng = &mut TestRng::new();
        let hash_addr: HashAddr = rng.gen();
        let entity_addr = EntityAddr::new_smart_contract(hash_addr);
        let entry_point_name = "ep1";
        let initial_state = create_entry_point(entity_addr, entry_point_name);
        let (contract_runtime, state_hash) = create_test_state(rng, initial_state);
        let request =
            EntryPointExistsRequest::new(state_hash, entry_point_name.to_string(), hash_addr);
        let res = contract_runtime
            .data_access_layer()
            .entry_point_exists(request);
        assert!(matches!(res, EntryPointExistsResult::Success));
    }

    #[test]
    fn fetching_enty_points_fetches_fail_when_asking_for_non_existing() {
        let rng = &mut TestRng::new();
        let hash_addr: HashAddr = rng.gen();
        let entity_addr = EntityAddr::new_smart_contract(hash_addr);
        let initial_state = create_entry_point(entity_addr, "ep1");
        let (contract_runtime, state_hash) = create_test_state(rng, initial_state);
        let request = EntryPointExistsRequest::new(state_hash, "ep2".to_string(), hash_addr);
        let res = contract_runtime
            .data_access_layer()
            .entry_point_exists(request);
        assert!(matches!(res, EntryPointExistsResult::ValueNotFound { .. }));
    }

    #[test]
    fn returns_trie_or_chunk() {
        let rng = &mut TestRng::new();
        let (contract_runtime, root_hash) =
            create_test_state(rng, create_test_pairs_with_large_data());

        // Expect `Trie` with NodePointer when asking with a root hash.
        let trie = read_trie(&contract_runtime, TrieOrChunkId(0, root_hash));
        assert!(matches!(trie, ValueOrChunk::Value(_)));

        // Expect another `Trie` with two LeafPointers.
        let trie = read_trie(
            &contract_runtime,
            TrieOrChunkId(0, extract_next_hash_from_trie(trie)),
        );
        assert!(matches!(trie, TrieOrChunk::Value(_)));

        // Now, the next hash will point to the actual leaf, which as we expect
        // contains large data, so we expect to get `ChunkWithProof`.
        let hash = extract_next_hash_from_trie(trie);
        let chunk = match read_trie(&contract_runtime, TrieOrChunkId(0, hash)) {
            TrieOrChunk::ChunkWithProof(chunk) => chunk,
            other => panic!("expected ChunkWithProof, got {:?}", other),
        };

        assert_eq!(chunk.proof().root_hash(), hash);

        // try to read all the chunks
        let count = chunk.proof().count();
        let mut chunks = vec![chunk];
        for i in 1..count {
            let chunk = match read_trie(&contract_runtime, TrieOrChunkId(i, hash)) {
                TrieOrChunk::ChunkWithProof(chunk) => chunk,
                other => panic!("expected ChunkWithProof, got {:?}", other),
            };
            chunks.push(chunk);
        }

        // there should be no chunk with index `count`
        let serialized_id = bincode::serialize(&TrieOrChunkId(count, hash)).unwrap();
        assert!(matches!(
            contract_runtime.fetch_trie_local(&serialized_id),
            Err(ContractRuntimeError::ChunkingError(
                ChunkingError::MerkleConstruction(_)
            ))
        ));

        // all chunks should be valid
        assert!(chunks.iter().all(|chunk| chunk.verify().is_ok()));

        let data: Vec<u8> = chunks
            .into_iter()
            .flat_map(|chunk| chunk.into_chunk())
            .collect();

        let trie: Trie<Key, StoredValue> =
            bytesrepr::deserialize(data).expect("trie should deserialize correctly");

        // should be deserialized to a leaf
        assert!(matches!(trie, Trie::Leaf { .. }));
    }
}
