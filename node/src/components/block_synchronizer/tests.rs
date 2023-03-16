pub(crate) mod test_utils;

use std::{
    collections::{BTreeMap, HashSet},
    iter,
    rc::Rc,
    time::Duration,
};

use casper_hashing::ChunkWithProof;
use casper_types::{
    bytesrepr::Bytes, system::auction::ValidatorWeights, testing::TestRng, EraId, TimeDiff,
};
use derive_more::From;
use rand::Rng;

use super::*;
use crate::{
    components::consensus::tests::utils::{
        ALICE_PUBLIC_KEY, ALICE_SECRET_KEY, BOB_PUBLIC_KEY, CAROL_PUBLIC_KEY,
    },
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    testing::test_block_builder::TestBlockBuilder,
    types::{TrieOrChunkId, ValueOrChunk},
    utils,
};
use assert_matches::assert_matches;

/// Event for the mock reactor.
#[derive(Debug, From)]
enum MockReactorEvent {
    BlockCompleteConfirmationRequest(BlockCompleteConfirmationRequest),
    BlockFetcherRequest(FetcherRequest<Block>),
    BlockHeaderFetcherRequest(FetcherRequest<BlockHeader>),
    LegacyDeployFetcherRequest(FetcherRequest<LegacyDeploy>),
    DeployFetcherRequest(FetcherRequest<Deploy>),
    FinalitySignatureFetcherRequest(FetcherRequest<FinalitySignature>),
    TrieOrChunkFetcherRequest(FetcherRequest<TrieOrChunk>),
    BlockExecutionResultsOrChunkFetcherRequest(FetcherRequest<BlockExecutionResultsOrChunk>),
    SyncLeapFetcherRequest(FetcherRequest<SyncLeap>),
    NetworkInfoRequest(NetworkInfoRequest),
    BlockAccumulatorRequest(BlockAccumulatorRequest),
    PeerBehaviorAnnouncement(PeerBehaviorAnnouncement),
    StorageRequest(StorageRequest),
    ContractRuntimeRequest(ContractRuntimeRequest),
    MakeBlockExecutableRequest(MakeBlockExecutableRequest),
    MetaBlockAnnouncement(MetaBlockAnnouncement),
    UpdateEraValidatorsRequest(UpdateEraValidatorsRequest),
}

impl From<FetcherRequest<ApprovalsHashes>> for MockReactorEvent {
    fn from(_req: FetcherRequest<ApprovalsHashes>) -> MockReactorEvent {
        unreachable!()
    }
}

struct MockReactor {
    scheduler: &'static Scheduler<MockReactorEvent>,
    effect_builder: EffectBuilder<MockReactorEvent>,
}

impl MockReactor {
    fn new() -> Self {
        let scheduler = utils::leak(Scheduler::new(QueueKind::weights()));
        let event_queue_handle = EventQueueHandle::without_shutdown(scheduler);
        let effect_builder = EffectBuilder::new(event_queue_handle);
        MockReactor {
            scheduler,
            effect_builder,
        }
    }

    fn effect_builder(&self) -> EffectBuilder<MockReactorEvent> {
        self.effect_builder
    }

    async fn crank(&self) -> MockReactorEvent {
        let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
        reactor_event
    }
}

// Create multiple random peers
fn random_peers(rng: &mut TestRng, num_random_peers: usize) -> HashSet<NodeId> {
    (0..num_random_peers)
        .into_iter()
        .map(|_| NodeId::random(rng))
        .collect()
}

fn random_test_trie(rng: &mut TestRng, size: usize) -> TrieRaw {
    let data: Vec<u8> = (0..size).into_iter().map(|_| rng.gen()).collect();
    TrieRaw::new(Bytes::from(data))
}

fn set_up_have_block_for_historical_builder(
    rng: &mut TestRng,
    root_trie_size: usize,
) -> (Block, BlockSynchronizer, Vec<NodeId>, TrieRaw) {
    // Set up a random block that we will use to test synchronization
    let root_trie = random_test_trie(rng, root_trie_size);
    let block = TestBlockBuilder::new()
        .state_root_hash(Digest::hash(root_trie.inner()))
        .deploys([Deploy::random(rng)].iter())
        .build(rng);

    // Set up a validator matrix for the era in which our test block was created
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    validator_matrix.register_validator_weights(
        block.header().era_id(),
        iter::once((ALICE_PUBLIC_KEY.clone(), 100.into())).collect(),
    );

    // Create a block synchronizer
    let mut block_synchronizer = BlockSynchronizer::new(
        Config::default()
            .with_peer_refresh_interval(TimeDiff::from(Duration::from_secs(10)))
            .with_max_parallel_trie_fetches(10),
        Arc::new(Chainspec::random(rng)),
        5,
        validator_matrix,
        &prometheus::Registry::new(),
    )
    .unwrap();

    // Generate more than peers to be selected for fetching global state data
    let num_peers = rng.gen_range(10..20);
    let peers: Vec<NodeId> = random_peers(rng, num_peers).iter().cloned().collect();

    // Set up the synchronizer for the test block such that the next step is getting global state
    block_synchronizer.register_block_by_hash(*block.hash(), true, true, false);
    assert!(block_synchronizer.historical.is_some()); // we only get global state on historical sync
    block_synchronizer.register_peers(*block.hash(), peers.clone());
    let historical_builder = block_synchronizer.historical.as_mut().unwrap();
    assert!(historical_builder
        .register_block_header(block.header().clone(), None)
        .is_ok());
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Generate a finality signature for the test block and register it
    let signature = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &Rc::new(ALICE_SECRET_KEY.clone()),
        ALICE_PUBLIC_KEY.clone(),
    );
    assert!(signature.is_verified().is_ok());
    assert!(historical_builder
        .register_finality_signature(signature, None)
        .is_ok());
    assert!(historical_builder.register_block(&block, None).is_ok());

    (block, block_synchronizer, peers, root_trie)
}

// Calls need_next for the block_synchronizer and processes the effects resulted returning a list of
// the new events that were generated
async fn need_next(
    rng: &mut TestRng,
    reactor: &MockReactor,
    block_synchronizer: &mut BlockSynchronizer,
    num_expected_events: usize,
) -> Vec<MockReactorEvent> {
    let mut effects = block_synchronizer.need_next(reactor.effect_builder(), rng);
    assert_eq!(effects.len(), num_expected_events);

    let mut events = Vec::new();
    for effect in effects.drain(0..) {
        tokio::spawn(async move { effect.await });
        let event = reactor.crank().await;
        events.push(event);
    }
    events
}

#[tokio::test]
async fn global_state_acquisition_is_created_for_historical_builder() {
    let mut rng = TestRng::new();

    let (block, block_synchronizer, _, _) = set_up_have_block_for_historical_builder(&mut rng, 64);

    let historical_builder = block_synchronizer.historical.as_ref().unwrap();
    let acq_state = historical_builder.block_acquisition_state();

    // The state of the acquisition should be `HaveBlock`
    // and a GlobalStateAcquisition object should have been created
    assert_matches!(acq_state, block_acquisition::BlockAcquisitionState::HaveBlock(_, _, _, ref global_state_acq) => {
        assert_eq!(
            global_state_acq.as_ref().unwrap().root_hash(),
            *block.state_root_hash()
        );
    });
}

#[tokio::test]
async fn global_state_trie_fetch_error_retriggers_fetch_for_same_trie() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    let (block, mut block_synchronizer, _, root_trie) =
        set_up_have_block_for_historical_builder(&mut rng, 64);

    // At this point, the next step the synchronizer takes should be to get global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Expect to get a request to fetch a trie
    let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Simulate that we did not get the trie
    let fetch_result: FetchResult<TrieOrChunk> = Err(FetcherError::TimedOut {
        id: expected_trie_or_chunk,
        peer: selected_peer,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Check if the peer that did not provide the trie is marked unreliable
    assert!(block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .peer_list()
        .is_peer_unreliable(&selected_peer));

    // Check that we got another request to fetch the trie since the last one failed
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    let peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // This time simulate a successful fetch
    let trie_or_chunk =
        Box::new(TrieOrChunk::new(*block.state_root_hash(), root_trie.into(), 0).unwrap());
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Check if the peer that did not provide the trie is marked unreliable
    assert!(block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .peer_list()
        .is_peer_reliable(&peer));

    // Now the global state acquisition logic will want to store the trie
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    let trie_to_put = assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
        assert_matches!(req, ContractRuntimeRequest::PutTrie { trie_bytes, responder: _ } => {
            trie_bytes
        })
    });

    // Check if global state acquisition is finished
    block_synchronizer.put_trie_result(
        *block.state_root_hash(),
        *block.state_root_hash(),
        trie_to_put,
        Ok(*block.state_root_hash()),
    );
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    assert_matches!(
        event,
        MockReactorEvent::ContractRuntimeRequest(
            ContractRuntimeRequest::GetExecutionResultsChecksum { .. }
        )
    );
}

#[tokio::test]
async fn global_state_sync_with_multiple_tries_has_correct_event_order() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    let (block, mut block_synchronizer, _, root_trie) =
        set_up_have_block_for_historical_builder(&mut rng, 64);

    // At this point, the next step the synchronizer takes should be to get global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Expect to get a request to fetch a trie
    let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
    let peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Check that the peer was marked as unknown before registering the fetch
    assert!(block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .peer_list()
        .is_peer_unknown(&peer));

    // Simulate a successful fetch
    let trie_or_chunk =
        Box::new(TrieOrChunk::new(*block.state_root_hash(), root_trie.into(), 0).unwrap());
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Check that the peer was marked as reliable after the successful fetch
    assert!(block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .peer_list()
        .is_peer_reliable(&peer));

    // Get next action
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Now the global state acquisition logic will want to store the trie
    let root_trie = assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
        assert_matches!(req, ContractRuntimeRequest::PutTrie { trie_bytes, responder: _ } => {
            trie_bytes
        })
    });

    // Simulate a failure to store the trie because it has missing children
    let missing_tries: Vec<TrieRaw> = (0..5)
        .into_iter()
        .map(|_| random_test_trie(&mut rng, 64))
        .collect();
    let missing_trie_nodes_hashes: Vec<Digest> = missing_tries
        .iter()
        .map(|trie| Digest::hash(trie.inner()))
        .collect();
    block_synchronizer.put_trie_result(
        *block.state_root_hash(),
        *block.state_root_hash(),
        root_trie.clone(),
        Err(engine_state::Error::MissingTrieNodeChildren(
            missing_trie_nodes_hashes.clone(),
        )),
    );

    // Check if fetches have been issued for the missing children
    let mut events = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 5).await;
    for event in events.drain(0..) {
        assert_matches!(
            event,
            MockReactorEvent::TrieOrChunkFetcherRequest(req) if missing_trie_nodes_hashes.contains(&req.id.trie_hash)
        );
    }

    // Simulate successful fetches for the missing children as requested above
    for (idx, hash) in missing_trie_nodes_hashes.iter().enumerate() {
        let trie_or_chunk =
            Box::new(TrieOrChunk::new(*hash, missing_tries[idx].clone().into(), 0).unwrap());
        let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
            peer,
            item: trie_or_chunk,
        });
        block_synchronizer.trie_or_chunk_fetched(
            *block.hash(),
            *block.state_root_hash(),
            *hash,
            fetch_result,
        );
        let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
            .await
            .remove(0);

        let trie_to_put = assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
            assert_matches!(req, ContractRuntimeRequest::PutTrie { trie_bytes, responder: _ } => {
                trie_bytes
            })
        });

        // Register the put_trie result
        block_synchronizer.put_trie_result(*block.state_root_hash(), *hash, trie_to_put, Ok(*hash));

        // No events will be triggered until the last trie fetch and successful put have been
        // completed
        if idx < 4 {
            need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 0).await;
        } else {
            // When all the missing children are stored, we should get a request to put the root
            // trie (which was awaiting for those children)
            let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
                .await
                .remove(0);
            assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
                assert_matches!(
                    req,
                    ContractRuntimeRequest::PutTrie { trie_bytes, responder: _ } if trie_bytes == root_trie
                );
            });
        }
    }

    // Simulate a successful put trie and check if the global state synchronization has ended
    block_synchronizer.put_trie_result(
        *block.state_root_hash(),
        *block.state_root_hash(),
        root_trie.clone(),
        Ok(*block.state_root_hash()),
    );
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    assert_matches!(
        event,
        MockReactorEvent::ContractRuntimeRequest(
            ContractRuntimeRequest::GetExecutionResultsChecksum { .. }
        )
    );
}

#[tokio::test]
async fn global_state_sync_multi_chunk_tries_successful() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    let (block, mut block_synchronizer, _, root_trie) =
        set_up_have_block_for_historical_builder(&mut rng, ChunkWithProof::CHUNK_SIZE_BYTES * 2);

    // At this point, the next step the synchronizer takes should be to get global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Expect to get a request to fetch a trie
    let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Simulate a successful fetch for the first chunk
    let trie_or_chunk =
        Box::new(TrieOrChunk::new(*block.state_root_hash(), root_trie.clone().into(), 0).unwrap());
    assert_matches!(
        trie_or_chunk.clone().into_value(),
        ValueOrChunk::ChunkWithProof(_)
    );
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer: selected_peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Expect to get a request to fetch the next chunk of the trie (chunk 1)
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    let expected_trie_or_chunk = TrieOrChunkId::new(1, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Simulate a successful fetch
    let trie_or_chunk =
        Box::new(TrieOrChunk::new(*block.state_root_hash(), root_trie.into(), 1).unwrap());
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer: selected_peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Acquisition should resume in a normal flow
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    assert_matches!(
        event,
        MockReactorEvent::ContractRuntimeRequest(ContractRuntimeRequest::PutTrie { .. })
    );
}

#[tokio::test]
async fn duplicate_global_state_trie_chunk_fetch_does_not_stall_acquisition() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    let (block, mut block_synchronizer, _, root_trie) =
        set_up_have_block_for_historical_builder(&mut rng, ChunkWithProof::CHUNK_SIZE_BYTES * 2);

    // At this point, the next step the synchronizer takes should be to get global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Expect to get a request to fetch a trie
    let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Simulate a successful fetch for the first chunk
    let trie_or_chunk =
        Box::new(TrieOrChunk::new(*block.state_root_hash(), root_trie.clone().into(), 0).unwrap());
    assert_matches!(
        trie_or_chunk.clone().into_value(),
        ValueOrChunk::ChunkWithProof(_)
    );
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer: selected_peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Expect to get a request to fetch the next chunk of the trie (chunk 1)
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    let expected_trie_or_chunk = TrieOrChunkId::new(1, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Simulate a duplicate fetch for chunk 0
    let trie_or_chunk =
        Box::new(TrieOrChunk::new(*block.state_root_hash(), root_trie.into(), 0).unwrap());
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer: selected_peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Expect to get a request to fetch chunk 1 again
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    let expected_trie_or_chunk = TrieOrChunkId::new(1, *block.state_root_hash());
    assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
    });
}

#[tokio::test]
async fn global_state_trie_fetch_validation_programming_error_aborts_historical_sync() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    let (block, mut block_synchronizer, _, root_trie) =
        set_up_have_block_for_historical_builder(&mut rng, ChunkWithProof::CHUNK_SIZE_BYTES * 2);

    // At this point, the next step the synchronizer takes should be to get global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Expect to get a request to fetch a trie
    let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Simulate a successful fetch for the first chunk (chunk 0)
    let trie_or_chunk =
        Box::new(TrieOrChunk::new(*block.state_root_hash(), root_trie.clone().into(), 0).unwrap());
    assert_matches!(
        trie_or_chunk.clone().into_value(),
        ValueOrChunk::ChunkWithProof(_)
    );
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer: selected_peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Expect to get a request to fetch chunk 1
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    let expected_trie_or_chunk = TrieOrChunkId::new(1, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Simulate a successful fetch for an invalid trie_or_chunk
    // This wouldn't be allowed by the fetcher validation logic
    // but we are forcing it here to test if the sync is aborted on error
    let trie_or_chunk = Box::new(
        TrieOrChunk::new(
            *block.state_root_hash(),
            random_test_trie(&mut rng, ChunkWithProof::CHUNK_SIZE_BYTES * 2).into(),
            1,
        )
        .unwrap(),
    );
    assert!(trie_or_chunk.validate(&EmptyValidationMetadata).is_err()); // actually invalid
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer: selected_peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Check if the synchronizer failed
    let historical_builder = block_synchronizer.historical.as_ref().unwrap();
    assert_matches!(
        historical_builder.block_acquisition_state(),
        block_acquisition::BlockAcquisitionState::Failed(..)
    );
}

#[tokio::test]
async fn peers_refreshed_when_repeatedly_failing_to_fetch_global_state() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    let (block, mut block_synchronizer, peers, _) =
        set_up_have_block_for_historical_builder(&mut rng, 64);

    // Try to fetch the root trie until we exhaust the whole peer set;
    // each time simulate a failed fetch
    for _ in 0..peers.len() {
        let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
            .await
            .remove(0);

        // Expect to get a request to fetch a trie
        let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
        let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
            assert_eq!(req.id, expected_trie_or_chunk);
            req.peer
        });

        // Check that the peer was marked as unknown
        assert!(block_synchronizer
            .historical
            .as_ref()
            .unwrap()
            .peer_list()
            .is_peer_unknown(&selected_peer));

        // Simulate that we did not get the trie
        let fetch_result: FetchResult<TrieOrChunk> = Err(FetcherError::Absent {
            id: expected_trie_or_chunk,
            peer: selected_peer,
        });
        block_synchronizer.trie_or_chunk_fetched(
            *block.hash(),
            *block.state_root_hash(),
            *block.state_root_hash(),
            fetch_result,
        );

        // Check if the peer that did not provide the trie is marked unreliable
        assert!(block_synchronizer
            .historical
            .as_ref()
            .unwrap()
            .peer_list()
            .is_peer_unreliable(&selected_peer));
    }

    // By this point all peers are unreliable since all requests to fetch from them have yielded
    // errors. The global state acquisition logic will continue to retry unreliable peers until
    // the peer list is refreshed.
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });
    assert!(block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .peer_list()
        .is_peer_unreliable(&selected_peer));

    // Simulate that we did not get the trie
    let fetch_result: FetchResult<TrieOrChunk> = Err(FetcherError::Absent {
        id: expected_trie_or_chunk,
        peer: selected_peer,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Wait for peer refresh interval to elapse
    std::thread::sleep(Duration::from_secs(10));

    // Now the block synchronizer should try to get new peers form both the accumulator and from the
    // peer set
    let events = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 2).await;
    assert_matches!(
        events[0],
        MockReactorEvent::NetworkInfoRequest(NetworkInfoRequest::FullyConnectedPeers { .. })
    );
    assert_matches!(
        events[1],
        MockReactorEvent::BlockAccumulatorRequest(BlockAccumulatorRequest::GetPeersForBlock { .. })
    );

    // Generate some more peers to register to the synchronizer
    let num_peers = rng.gen_range(10..20);
    let peers: Vec<NodeId> = random_peers(&mut rng, num_peers).iter().cloned().collect();
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    // Now the synchronizer should resume fetching tries for global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Check that the peer was selected from the newly registered peer set
    assert!(peers.contains(&selected_peer));
    assert!(block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .peer_list()
        .is_peer_unknown(&selected_peer));
}

#[tokio::test]
async fn global_state_sync_does_not_exceed_maximum_parallel_trie_fetches() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    let (block, mut block_synchronizer, _, root_trie) =
        set_up_have_block_for_historical_builder(&mut rng, 64);

    // At this point, the next step the synchronizer takes should be to get global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Expect to get a request to fetch a trie
    let expected_trie_or_chunk = TrieOrChunkId::new(0, *block.state_root_hash());
    let peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Check that the peer was marked as unknown
    assert!(block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .peer_list()
        .is_peer_unknown(&peer));

    // Simulate a successful fetch
    let trie_or_chunk =
        Box::new(TrieOrChunk::new(*block.state_root_hash(), root_trie.into(), 0).unwrap());
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *block.state_root_hash(),
        *block.state_root_hash(),
        fetch_result,
    );

    // Check that the peer was marked as reliable
    assert!(block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .peer_list()
        .is_peer_reliable(&peer));

    // Get next action
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Now the global state acquisition logic will want to store the trie
    let root_trie = assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
        assert_matches!(req, ContractRuntimeRequest::PutTrie { trie_bytes, responder: _ } => {
            trie_bytes
        })
    });

    // Generate a list of missing children which is larger than the number of maximum parallel trie
    // fetches allowed
    let missing_tries: Vec<TrieRaw> = (0..15)
        .into_iter()
        .map(|_| random_test_trie(&mut rng, 64))
        .collect();
    let missing_trie_nodes_hashes: Vec<Digest> = missing_tries
        .iter()
        .map(|trie| Digest::hash(trie.inner()))
        .collect();
    block_synchronizer.put_trie_result(
        *block.state_root_hash(),
        *block.state_root_hash(),
        root_trie.clone(),
        Err(engine_state::Error::MissingTrieNodeChildren(
            missing_trie_nodes_hashes.clone(),
        )),
    );

    // Check the number of fetches have been issued for the missing children
    // This should be the max_parallel_trie_fetches limit
    let mut events = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 10).await;
    for event in events.drain(0..) {
        assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) if missing_trie_nodes_hashes.contains(&req.id.trie_hash));
    }
}

#[tokio::test]
async fn sync_validator_weights_from_global_state() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    // Set up a validator matrix with one era initialized
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    validator_matrix.register_validator_weights(
        EraId::from(2),
        iter::once((ALICE_PUBLIC_KEY.clone(), 100.into())).collect(),
    );

    // Create a block synchronizer
    let mut block_synchronizer = BlockSynchronizer::new(
        Config::default()
            .with_peer_refresh_interval(TimeDiff::from(Duration::from_secs(10)))
            .with_max_parallel_trie_fetches(10),
        Arc::new(Chainspec::random(&mut rng)),
        5,
        validator_matrix,
        prometheus::default_registry(),
    )
    .unwrap();

    // Create 2 switch blocks
    let parent_root_trie = random_test_trie(&mut rng, 64);
    let parent_block = TestBlockBuilder::new()
        .state_root_hash(Digest::hash(parent_root_trie.inner()))
        .deploys([Deploy::random(&mut rng)].iter())
        .era(3)
        .height(39)
        .switch_block(true)
        .build(&mut rng);

    let root_trie = random_test_trie(&mut rng, 64);
    let block = TestBlockBuilder::new()
        .state_root_hash(Digest::hash(root_trie.inner()))
        .deploys([Deploy::random(&mut rng)].iter())
        .era(4)
        .height(49)
        .switch_block(true)
        .parent_hash(*parent_block.hash())
        .build(&mut rng);

    // Generate and register some peers to be used by the synchronizer
    let num_peers = rng.gen_range(10..20);
    let peers: Vec<NodeId> = random_peers(&mut rng, num_peers).iter().cloned().collect();

    // Set up the synchronizer for the test block and mark that it should get the missing era
    // validator weights from global state
    block_synchronizer.register_block_by_hash(*block.hash(), true, true, true);
    assert!(block_synchronizer.historical.is_some()); // we only get global state on historical sync
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    // Expect header fetch requests
    let mut events = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 5).await;
    for event in events.drain(0..) {
        assert_matches!(event, MockReactorEvent::BlockHeaderFetcherRequest(req) if req.id == *block.hash());
    }

    // Register the block header
    let fetch_result: FetchResult<BlockHeader> = Ok(FetchedData::FromStorage {
        item: Box::new(block.header().clone()),
    });
    block_synchronizer.block_header_fetched(fetch_result);

    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Since there are no era validator weights for the block era, the synchronizer should try and
    // get them from global state
    assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
        assert_matches!(req, ContractRuntimeRequest::GetEraValidators{request, responder: _} => {
            assert_eq!(request.state_hash(), *block.state_root_hash());
            assert_eq!(request.protocol_version(), block.protocol_version());
        })
    });

    // Simulate that we have the global state for this block and return some validator weights for
    // its era
    let era_validator_weights: ValidatorWeights = BTreeMap::from([
        (BOB_PUBLIC_KEY.clone(), 100.into()),
        (CAROL_PUBLIC_KEY.clone(), 200.into()),
    ]);
    let era_validators_get_response = Ok(BTreeMap::from([
        (EraId::from(5), era_validator_weights.clone()),
        (EraId::from(6), era_validator_weights),
    ]));
    block_synchronizer.register_era_validators_from_contract_runtime(
        *block.state_root_hash(),
        era_validators_get_response,
    );

    // Next we require the parent of the block to check its global state for era validators
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);
    assert_matches!(event, MockReactorEvent::StorageRequest(req) => {
        assert_matches!(req, StorageRequest::GetBlockHeader{block_hash, only_from_available_block_range, responder: _} => {
            assert_eq!(block_hash, *parent_block.hash());
            assert!(!only_from_available_block_range);
        })
    });

    block_synchronizer.register_block_header_requested_from_storage(
        *block.hash(),
        Some(parent_block.clone().take_header()),
    );

    // Since there are no era validator weights for the parent block era, the synchronizer should
    // try and get them from global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
        assert_matches!(req, ContractRuntimeRequest::GetEraValidators{request, responder: _} => {
            assert_eq!(request.state_hash(), *parent_block.state_root_hash());
            assert_eq!(request.protocol_version(), parent_block.protocol_version());
        })
    });

    // Simulate that we don't have the global state for the parent block
    block_synchronizer.register_era_validators_from_contract_runtime(
        *parent_block.state_root_hash(),
        Err(EraValidatorsGetError::RootNotFound),
    );

    // Expect to get a request to fetch a trie for the parent global state
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    let expected_trie_or_chunk = TrieOrChunkId::new(0, *parent_block.state_root_hash());
    let selected_peer = assert_matches!(event, MockReactorEvent::TrieOrChunkFetcherRequest(req) => {
        assert_eq!(req.id, expected_trie_or_chunk);
        req.peer
    });

    // Simulate a successful fetch for the first chunk
    let trie_or_chunk = Box::new(
        TrieOrChunk::new(
            *parent_block.state_root_hash(),
            parent_root_trie.clone().into(),
            0,
        )
        .unwrap(),
    );
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
        peer: selected_peer,
        item: trie_or_chunk,
    });
    block_synchronizer.trie_or_chunk_fetched(
        *block.hash(),
        *parent_block.state_root_hash(),
        *parent_block.state_root_hash(),
        fetch_result,
    );

    // Now the global state acquisition logic will want to store the trie
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    let trie_to_put = assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
        assert_matches!(req, ContractRuntimeRequest::PutTrie { trie_bytes, responder: _ } => {
            trie_bytes
        })
    });

    // Check if global state acquisition is finished
    block_synchronizer.put_trie_result(
        *parent_block.state_root_hash(),
        *parent_block.state_root_hash(),
        trie_to_put,
        Ok(*block.state_root_hash()),
    );
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    // Try to get the era validators for the parent block again
    assert_matches!(event, MockReactorEvent::ContractRuntimeRequest(req) => {
        assert_matches!(req, ContractRuntimeRequest::GetEraValidators{request, responder: _} => {
            assert_eq!(request.state_hash(), *parent_block.state_root_hash());
            assert_eq!(request.protocol_version(), parent_block.protocol_version());
        })
    });

    // Register the era validators received from contract runtime
    let era_4_validator_weights: ValidatorWeights = BTreeMap::from([
        (BOB_PUBLIC_KEY.clone(), 100.into()),
        (CAROL_PUBLIC_KEY.clone(), 200.into()),
    ]);
    let era_validators_get_response = Ok(BTreeMap::from([
        (EraId::from(4), era_4_validator_weights.clone()),
        (EraId::from(5), era_4_validator_weights),
    ]));
    block_synchronizer.register_era_validators_from_contract_runtime(
        *parent_block.state_root_hash(),
        era_validators_get_response,
    );

    // Expect a request to update the validator matrix
    let event = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1)
        .await
        .remove(0);

    let validator_weights = assert_matches!(
        event,
        MockReactorEvent::UpdateEraValidatorsRequest(req) => {
            assert_matches!(req, UpdateEraValidatorsRequest {era_id, validators_to_register } => {
                assert_eq!(era_id, EraId::from(4));
                validators_to_register
            })
        }
    );

    // When the request reaches the main reactor, it will update the validator matrix and notify the
    // block synchronizer
    block_synchronizer
        .validator_matrix
        .register_validator_weights(EraId::from(4), validator_weights);
    let mut effects = block_synchronizer.handle_validators(mock_reactor.effect_builder(), &mut rng);

    // Check that the next step was triggered in the block synchronizer
    for effect in effects.drain(0..) {
        tokio::spawn(async move { effect.await });
        let event = mock_reactor.crank().await;
        assert_matches!(event, MockReactorEvent::FinalitySignatureFetcherRequest(req) => {
                assert_eq!(req.id.block_hash, *block.hash());
                assert_eq!(req.id.era_id, EraId::from(4));
        });
    }
}
