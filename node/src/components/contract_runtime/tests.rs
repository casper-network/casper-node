use std::{sync::Arc, time::Duration};

use derive_more::{Display, From};
use prometheus::Registry;
use rand::RngCore;
use serde::Serialize;
use tempfile::TempDir;

use casper_types::{
    bytesrepr::Bytes, runtime_args, BlockHash, Chainspec, ChainspecRawBytes, Deploy, Digest, EraId,
    ExecutableDeployItem, PublicKey, SecretKey, TimeDiff, Timestamp, U512,
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
        BlockPayload, ExecutableBlock, FinalizedBlock, InternalEraReport, MetaBlockState,
        TransactionHashWithApprovals,
    },
    utils::{Loadable, WithDir, RESOURCES_PATH},
    NodeRng,
};

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

struct Reactor {
    storage: Storage,
    contract_runtime: ContractRuntime,
    _storage_tempdir: TempDir,
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = Config;
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
        )
        .unwrap();

        let contract_runtime =
            ContractRuntime::new(storage.root_path(), &config, chainspec, registry)?;

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
        .set_initial_state(initial_pre_state);

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
          "amount" => U512::from(chainspec.system_costs_config.wasmless_transfer_cost()),
        },
    };

    let txns: Vec<Transaction> = std::iter::repeat_with(|| {
        let target_public_key = PublicKey::random(rng);
        let session = ExecutableDeployItem::Transfer {
            args: runtime_args! {
              "amount" => U512::from(chainspec.transaction_config.native_transfer_minimum_motes),
              "target" => target_public_key,
              "id" => Some(9_u64),
            },
        };
        Transaction::Deploy(Deploy::new(
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
    let block_payload = BlockPayload::new(
        txns.iter()
            .map(TransactionHashWithApprovals::from)
            .collect(),
        vec![],
        vec![],
        vec![],
        vec![],
        Default::default(),
        true,
    );
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
        .set_initial_state(ExecutionPreState::new(
            next_block_height,
            Digest::hash(rng.next_u64().to_le_bytes()),
            BlockHash::random(rng),
            Digest::hash(rng.next_u64().to_le_bytes()),
        ));

    runner
        .crank_until(rng, execution_completed, TEST_TIMEOUT)
        .await;

    // Check that the next block height expected by the contract runtime is `next_block_height` and
    // not 3.
    assert_eq!(
        runner
            .reactor()
            .inner()
            .contract_runtime
            .execution_pre_state
            .lock()
            .unwrap()
            .next_block_height(),
        next_block_height
    );
}

#[cfg(test)]
mod trie_chunking_tests {
    use std::sync::Arc;

    use casper_storage::global_state::{state::StateProvider, trie::Trie};
    use casper_types::{
        account::AccountHash,
        bytesrepr,
        execution::{Transform, TransformKind},
        global_state::Pointer,
        testing::TestRng,
        ActivationPoint, CLValue, Chainspec, ChunkWithProof, CoreConfig, Digest, EraId, Key,
        ProtocolConfig, StoredValue, TimeDiff, DEFAULT_FEE_HANDLING, DEFAULT_REFUND_HANDLING,
    };
    use prometheus::Registry;
    use tempfile::tempdir;

    use crate::{
        components::fetcher::FetchResponse,
        contract_runtime::ContractRuntimeError,
        types::{ChunkingError, TrieOrChunk, TrieOrChunkId, ValueOrChunk},
    };

    use super::{Config as ContractRuntimeConfig, ContractRuntime};

    #[derive(Debug, Clone)]
    struct TestPair(Key, StoredValue);

    // Creates the test pairs that contain data of size
    // greater than the chunk limit.
    fn create_test_pairs_with_large_data() -> [TestPair; 2] {
        let val = CLValue::from_t(
            String::from_utf8(vec![b'a'; ChunkWithProof::CHUNK_SIZE_BYTES * 2]).unwrap(),
        )
        .unwrap();
        [
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
    fn create_test_state(rng: &mut TestRng, test_pair: [TestPair; 2]) -> (ContractRuntime, Digest) {
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
        let empty_state_root = contract_runtime.engine_state().get_state().empty_root();
        let mut effects = casper_types::execution::Effects::new();
        for TestPair(key, value) in test_pair {
            effects.push(Transform::new(key, TransformKind::Write(value)));
        }
        let post_state_hash = contract_runtime
            .engine_state()
            .commit_effects(empty_state_root, effects)
            .expect("applying effects to succeed");
        (contract_runtime, post_state_hash)
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
