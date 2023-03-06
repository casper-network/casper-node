use std::{collections::HashSet, iter, rc::Rc, time::Duration};

use casper_types::testing::TestRng;
use derive_more::From;
use rand::{seq::IteratorRandom, Rng};

use super::*;
use crate::{
    components::consensus::tests::utils::{ALICE_PUBLIC_KEY, ALICE_SECRET_KEY},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    utils,
};

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
    TrieAccumulatorRequest(TrieAccumulatorRequest),
    ContractRuntimeRequest(ContractRuntimeRequest),
    SyncGlobalStateRequest(SyncGlobalStateRequest),
    MakeBlockExecutableRequest(MakeBlockExecutableRequest),
    MetaBlockAnnouncement(MetaBlockAnnouncement),
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

fn check_sync_global_state_event(event: MockReactorEvent, block: &Block) -> HashSet<NodeId> {
    assert!(matches!(
        event,
        MockReactorEvent::SyncGlobalStateRequest { .. }
    ));
    let global_sync_request = match event {
        MockReactorEvent::SyncGlobalStateRequest(req) => req,
        _ => unreachable!(),
    };
    assert_eq!(global_sync_request.block_hash, *block.hash());
    assert_eq!(
        global_sync_request.state_root_hash,
        *block.state_root_hash()
    );

    global_sync_request.peers
}

#[tokio::test]
async fn global_state_sync_wont_stall_with_bad_peers() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();

    // Set up a random block that we will use to test synchronization
    let block = Block::random(&mut rng);

    // Set up a validator matrix for the era in which our test block was created
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    validator_matrix.register_validator_weights(
        block.header().era_id(),
        iter::once((ALICE_PUBLIC_KEY.clone(), 100.into())).collect(),
    );

    // Create a block synchronizer with a maximum of 5 simultaneous peers
    let mut block_synchronizer = BlockSynchronizer::new(
        Config::default(),
        Arc::new(Chainspec::random(&mut rng)),
        5,
        validator_matrix,
        prometheus::default_registry(),
    )
    .unwrap();

    // Generate more than 5 peers to see if the peer list changes after a global state sync error
    let num_peers = rng.gen_range(10..20);
    let peers: Vec<NodeId> = random_peers(&mut rng, num_peers).iter().cloned().collect();

    // Set up the synchronizer for the test block such that the next step is getting global state
    block_synchronizer.register_block_by_hash(*block.hash(), true, true);
    assert!(block_synchronizer.historical.is_some()); // we only get global state on historical sync
    block_synchronizer.register_peers(*block.hash(), peers);
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

    // At this point, the next step the synchronizer takes should be to get global state
    let mut effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(effects.len(), 1);
    tokio::spawn(async move { effects.remove(0).await });
    let event = mock_reactor.crank().await;

    // Expect a `SyncGlobalStateRequest` for the `GlobalStateSynchronizer`
    // The peer list that the GlobalStateSynchronizer will use to fetch the tries
    let first_peer_set = check_sync_global_state_event(event, &block);

    // Wait for the latch to reset
    std::thread::sleep(Duration::from_secs(6));

    // Simulate an error form the global_state_synchronizer;
    // make it seem that the `TrieAccumulator` did not find the required tries on any of the peers
    block_synchronizer.global_state_synced(
        *block.hash(),
        Err(GlobalStateSynchronizerError::TrieAccumulator(
            first_peer_set.iter().cloned().collect(),
        )),
    );

    // At this point we expect that another request for the global state would be made,
    // this time with other peers
    let mut effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(effects.len(), 1);
    tokio::spawn(async move { effects.remove(0).await });
    let event = mock_reactor.crank().await;
    let second_peer_set = check_sync_global_state_event(event, &block);

    // Check if the peers are the same as the ones in the previous request;
    // they should be different since we have enough peers registered that we have not tried
    for peer in second_peer_set.iter() {
        assert!(!first_peer_set.contains(peer))
    }

    // Wait for the latch to reset
    std::thread::sleep(Duration::from_secs(6));

    // Simulate a successful global state sync;
    // Although the request was successful, some peers did not have the data.
    let unreliable_peers = second_peer_set.into_iter().choose_multiple(&mut rng, 2);
    block_synchronizer.global_state_synced(
        *block.hash(),
        Ok(GlobalStateSynchronizerResponse::new(
            *block.state_root_hash(),
            unreliable_peers.clone(),
        )),
    );
    let mut effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(effects.len(), 1);
    tokio::spawn(async move { effects.remove(0).await });
    let event = mock_reactor.crank().await;

    // Synchronizer should have progressed
    assert!(false == matches!(event, MockReactorEvent::SyncGlobalStateRequest { .. }));

    // Check if the peers returned by the `GlobalStateSynchronizer` in the response were marked
    // unreliable.
    for peer in unreliable_peers.iter() {
        assert!(block_synchronizer
            .historical
            .as_ref()
            .unwrap()
            .peer_list()
            .is_peer_unreliable(peer));
    }
}
