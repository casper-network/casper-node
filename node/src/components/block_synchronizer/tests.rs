pub(crate) mod test_utils;

use std::{
    cmp::min,
    collections::{BTreeMap, VecDeque},
    iter,
    time::Duration,
};

use assert_matches::assert_matches;
use derive_more::From;
use num_rational::Ratio;
use rand::{seq::IteratorRandom, Rng};

use casper_storage::global_state::trie::merkle_proof::TrieMerkleProof;
use casper_types::{
    testing::TestRng, AccessRights, BlockV2, CLValue, Chainspec, DeployId, EraId, Key,
    LegacyRequiredFinality, ProtocolVersion, PublicKey, SecretKey, StoredValue, TestBlockBuilder,
    TimeDiff, URef, U512,
};

use super::*;
use crate::{
    components::{
        block_synchronizer::block_acquisition::BlockAcquisitionState,
        consensus::tests::utils::{ALICE_PUBLIC_KEY, ALICE_SECRET_KEY},
    },
    effect::Effect,
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    tls::KeyFingerprint,
    types::{BlockExecutionResultsOrChunkId, ValueOrChunk},
    utils,
};

const MAX_SIMULTANEOUS_PEERS: u8 = 5;
const TEST_LATCH_RESET_INTERVAL_MILLIS: u64 = 5;
const SHOULD_FETCH_EXECUTION_STATE: bool = true;
const STRICT_FINALITY_REQUIRED_VERSION: ProtocolVersion = ProtocolVersion::from_parts(1, 5, 0);

/// Event for the mock reactor.
#[derive(Debug, From)]
enum MockReactorEvent {
    MarkBlockCompletedRequest(MarkBlockCompletedRequest),
    BlockFetcherRequest(FetcherRequest<Block>),
    BlockHeaderFetcherRequest(FetcherRequest<BlockHeader>),
    LegacyDeployFetcherRequest(FetcherRequest<LegacyDeploy>),
    DeployFetcherRequest(FetcherRequest<Deploy>),
    FinalitySignatureFetcherRequest(FetcherRequest<FinalitySignature>),
    TrieOrChunkFetcherRequest(FetcherRequest<TrieOrChunk>),
    BlockExecutionResultsOrChunkFetcherRequest(FetcherRequest<BlockExecutionResultsOrChunk>),
    SyncLeapFetcherRequest(FetcherRequest<SyncLeap>),
    ApprovalsHashesFetcherRequest(FetcherRequest<ApprovalsHashes>),
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

struct MockReactor {
    scheduler: &'static Scheduler<MockReactorEvent>,
    effect_builder: EffectBuilder<MockReactorEvent>,
}

impl MockReactor {
    fn new() -> Self {
        let scheduler = utils::leak(Scheduler::new(QueueKind::weights(), None));
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

    async fn process_effects(
        &self,
        effects: impl IntoIterator<Item = Effect<Event>>,
    ) -> Vec<MockReactorEvent> {
        let mut events = Vec::new();
        for effect in effects {
            tokio::spawn(async move { effect.await });
            let event = self.crank().await;
            events.push(event);
        }
        events
    }
}

struct TestEnv {
    block: BlockV2,
    validator_keys: Vec<Arc<SecretKey>>,
    peers: Vec<NodeId>,
}

// Utility struct used to generate common test artifacts
impl TestEnv {
    // Replaces the test block with the one provided as parameter
    fn with_block(self, block: BlockV2) -> Self {
        Self {
            block,
            validator_keys: self.validator_keys,
            peers: self.peers,
        }
    }

    fn block(&self) -> &BlockV2 {
        &self.block
    }

    fn validator_keys(&self) -> &Vec<Arc<SecretKey>> {
        &self.validator_keys
    }

    fn peers(&self) -> &Vec<NodeId> {
        &self.peers
    }

    // Generates a `ValidatorMatrix` that has the validators for the era of the test block
    // All validators have equal weights
    fn gen_validator_matrix(&self) -> ValidatorMatrix {
        let validator_weights: BTreeMap<PublicKey, U512> = self
            .validator_keys
            .iter()
            .map(|key| (PublicKey::from(key.as_ref()), 100.into())) // we give each validator equal weight
            .collect();

        assert_eq!(validator_weights.len(), self.validator_keys.len());

        // Set up a validator matrix for the era in which our test block was created
        let mut validator_matrix = ValidatorMatrix::new(
            Ratio::new(1, 3),
            None,
            EraId::from(0),
            self.validator_keys[0].clone(),
            PublicKey::from(self.validator_keys[0].as_ref()),
            1,
        );
        validator_matrix.register_validator_weights(self.block.era_id(), validator_weights);

        validator_matrix
    }

    fn random(rng: &mut TestRng) -> TestEnv {
        let num_validators: usize = rng.gen_range(3..100);
        let validator_keys: Vec<_> = iter::repeat_with(|| Arc::new(SecretKey::random(rng)))
            .take(num_validators)
            .collect();

        let num_peers = rng.gen_range(10..20);

        TestEnv {
            block: TestBlockBuilder::new().build(rng),
            validator_keys,
            peers: iter::repeat(())
                .take(num_peers)
                .map(|_| NodeId::from(rng.gen::<KeyFingerprint>()))
                .collect(),
        }
    }
}

fn check_sync_global_state_event(event: MockReactorEvent, block: &BlockV2) {
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
}

// Calls need_next for the block_synchronizer and processes the effects resulted returning a list of
// the new events that were generated
async fn need_next(
    rng: &mut TestRng,
    reactor: &MockReactor,
    block_synchronizer: &mut BlockSynchronizer,
    num_expected_events: u8,
) -> Vec<MockReactorEvent> {
    let effects = block_synchronizer.need_next(reactor.effect_builder(), rng);
    assert_eq!(effects.len() as u8, num_expected_events);
    reactor.process_effects(effects).await
}

fn register_multiple_signatures<'a, I: IntoIterator<Item = &'a Arc<SecretKey>>>(
    builder: &mut BlockBuilder,
    block: &BlockV2,
    validator_keys_iter: I,
) {
    for secret_key in validator_keys_iter {
        // Register a finality signature
        let signature =
            FinalitySignature::create(*block.hash(), block.era_id(), secret_key.as_ref());
        assert!(signature.is_verified().is_ok());
        assert!(builder.register_finality_signature(signature, None).is_ok());
    }
}

fn dummy_merkle_proof() -> TrieMerkleProof<Key, StoredValue> {
    TrieMerkleProof::new(
        URef::new([255; 32], AccessRights::NONE).into(),
        StoredValue::CLValue(CLValue::from_t(()).unwrap()),
        VecDeque::new(),
    )
}

trait OneExt: IntoIterator {
    fn try_one(self) -> Option<Self::Item>;
    fn one(self) -> Self::Item;
}

impl<I: IntoIterator> OneExt for I {
    fn try_one(self) -> Option<Self::Item> {
        let mut it = self.into_iter();
        let first = it.next()?;

        it.next().is_none().then_some(first)
    }

    #[track_caller]
    fn one(self) -> Self::Item {
        let mut it = self.into_iter();
        let first = it
            .next()
            .expect("no element in the iterator, but 1 was expected");

        if it.next().is_some() {
            panic!("more that 1 element in the iterator, but 1 was expected")
        }

        first
    }
}

#[cfg(test)]
impl BlockSynchronizer {
    fn new_initialized(
        rng: &mut TestRng,
        validator_matrix: ValidatorMatrix,
        config: Config,
    ) -> BlockSynchronizer {
        let mut block_synchronizer = BlockSynchronizer::new(
            config,
            Arc::new(Chainspec::random(rng)),
            MAX_SIMULTANEOUS_PEERS,
            validator_matrix,
            &prometheus::Registry::new(),
        )
        .expect("Failed to create BlockSynchronizer");

        <BlockSynchronizer as InitializedComponent<MainEvent>>::set_state(
            &mut block_synchronizer,
            ComponentState::Initialized,
        );

        block_synchronizer
    }

    fn with_legacy_finality(mut self, legacy_required_finality: LegacyRequiredFinality) -> Self {
        let core_config = &mut Arc::get_mut(&mut self.chainspec).unwrap().core_config;
        core_config.start_protocol_version_with_strict_finality_signatures_required =
            STRICT_FINALITY_REQUIRED_VERSION;
        core_config.legacy_required_finality = legacy_required_finality;

        self
    }

    fn forward_builder(&self) -> &BlockBuilder {
        self.forward.as_ref().expect("Forward builder missing")
    }
}

/// Returns the number of validators that need a signature for a weak finality of 1/3.
fn weak_finality_threshold(n: usize) -> usize {
    n / 3 + 1
}

/// Returns the number of validators that need a signature for a strict finality of 2/3.
fn strict_finality_threshold(n: usize) -> usize {
    n * 2 / 3 + 1
}

fn latch_inner_check(builder: Option<&BlockBuilder>, expected: bool, msg: &str) {
    assert_eq!(
        builder.expect("builder should exist").latched(),
        expected,
        "{}",
        msg
    );
}

fn latch_count_check(builder: Option<&BlockBuilder>, expected: u8, msg: &str) {
    assert_eq!(
        builder.expect("builder should exist").latch_count(),
        expected,
        "{}",
        msg
    );
}

fn need_next_inner_check(
    builder: Option<&mut BlockBuilder>,
    rng: &mut TestRng,
    expected: NeedNext,
    msg: &str,
) {
    let need_next = builder
        .expect("should exist")
        .block_acquisition_action(rng, MAX_SIMULTANEOUS_PEERS)
        .need_next();
    assert_eq!(need_next, expected, "{}", msg);
}

#[tokio::test]
async fn global_state_sync_wont_stall_with_bad_peers() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .random_deploys(1, &mut rng)
            .build(&mut rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let cfg = Config {
        latch_reset_interval: TimeDiff::from_millis(TEST_LATCH_RESET_INTERVAL_MILLIS),
        ..Default::default()
    };
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, cfg);

    // Set up the synchronizer for the test block such that the next step is getting global state
    block_synchronizer.register_block_by_hash(*block.hash(), true);
    assert!(
        block_synchronizer.historical.is_some(),
        "we only get global state on historical sync"
    );
    block_synchronizer.register_peers(*block.hash(), peers.clone());
    let historical_builder = block_synchronizer.historical.as_mut().unwrap();
    assert!(
        historical_builder
            .register_block_header(block.header().clone(), None)
            .is_ok(),
        "historical builder should register header"
    );
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(
        historical_builder
            .register_block(block.into(), None)
            .is_ok(),
        "should register block"
    );
    // Register the remaining signatures to reach strict finality
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .skip(weak_finality_threshold(validators_secret_keys.len())),
    );

    // At this point, the next step the synchronizer takes should be to get global state
    let mut effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(
        effects.len(),
        1,
        "need next should have 1 effect at this step, not {}",
        effects.len()
    );
    tokio::spawn(async move { effects.remove(0).await });
    let event = mock_reactor.crank().await;

    // Expect a `SyncGlobalStateRequest` for the `GlobalStateSynchronizer`
    // The peer list that the GlobalStateSynchronizer will use to fetch the tries
    let first_peer_set = peers.iter().copied().choose_multiple(&mut rng, 4);
    check_sync_global_state_event(event, block);

    // Wait for the latch to reset
    tokio::time::sleep(Duration::from_millis(TEST_LATCH_RESET_INTERVAL_MILLIS * 2)).await;

    // Simulate an error form the global_state_synchronizer;
    // make it seem that the `TrieAccumulator` did not find the required tries on any of the peers
    block_synchronizer.global_state_synced(
        *block.hash(),
        Err(GlobalStateSynchronizerError::TrieAccumulator(
            first_peer_set.to_vec(),
        )),
    );

    // At this point we expect that another request for the global state would be made,
    // this time with other peers
    let mut effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(
        effects.len(),
        1,
        "need next should still have 1 effect at this step, not {}",
        effects.len()
    );
    tokio::spawn(async move { effects.remove(0).await });
    let event = mock_reactor.crank().await;

    let second_peer_set = peers.iter().copied().choose_multiple(&mut rng, 4);
    check_sync_global_state_event(event, block);

    // Wait for the latch to reset
    tokio::time::sleep(Duration::from_millis(TEST_LATCH_RESET_INTERVAL_MILLIS * 2)).await;

    // Simulate a successful global state sync;
    // Although the request was successful, some peers did not have the data.
    let unreliable_peers = second_peer_set.into_iter().choose_multiple(&mut rng, 2);
    block_synchronizer.global_state_synced(
        *block.hash(),
        Ok(GlobalStateSynchronizerResponse::new(
            (*block.state_root_hash()).into(),
            unreliable_peers.clone(),
        )),
    );
    let mut effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(
        effects.len(),
        1,
        "need next should still have 1 effect after global state sync'd, not {}",
        effects.len()
    );
    tokio::spawn(async move { effects.remove(0).await });
    let event = mock_reactor.crank().await;

    assert!(
        false == matches!(event, MockReactorEvent::SyncGlobalStateRequest { .. }),
        "synchronizer should have progressed"
    );

    // Check if the peers returned by the `GlobalStateSynchronizer` in the response were marked
    // unreliable.
    for peer in unreliable_peers.iter() {
        assert!(
            block_synchronizer
                .historical
                .as_ref()
                .unwrap()
                .peer_list()
                .is_peer_unreliable(peer),
            "{} should be marked unreliable",
            peer
        );
    }
}

#[tokio::test]
async fn synchronizer_doesnt_busy_loop_without_peers() {
    fn check_need_peer_events(expected_block_hash: BlockHash, events: Vec<MockReactorEvent>) {
        // Explicitly verify the two effects are indeed asking networking and accumulator for peers.
        assert_matches!(
            events[0],
            MockReactorEvent::NetworkInfoRequest(NetworkInfoRequest::FullyConnectedPeers {
                count,
                ..
            }) if count == MAX_SIMULTANEOUS_PEERS as usize
        );
        assert_matches!(
            events[1],
            MockReactorEvent::BlockAccumulatorRequest(BlockAccumulatorRequest::GetPeersForBlock {
                block_hash,
                ..
            }) if block_hash == expected_block_hash
        );
    }

    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .random_deploys(1, &mut rng)
            .build(&mut rng),
    );
    let block = test_env.block();
    let block_hash = *block.hash();
    let validator_matrix = test_env.gen_validator_matrix();
    let cfg = Config {
        latch_reset_interval: TimeDiff::from_millis(TEST_LATCH_RESET_INTERVAL_MILLIS),
        ..Default::default()
    };
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, cfg);

    block_synchronizer.register_block_by_hash(block_hash, true);

    latch_inner_check(
        block_synchronizer.historical.as_ref(),
        false,
        "initial set up, should not be latched",
    );

    {
        // We registered no peers, so we need peers
        need_next_inner_check(
            block_synchronizer.historical.as_mut(),
            &mut rng,
            NeedNext::Peers(block_hash),
            "should need peers",
        );

        // We registered no peers, so the synchronizer should ask for peers.
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::Request(BlockSynchronizerRequest::NeedNext),
        );
        assert_eq!(effects.len(), 2, "we should ask for peers from both networking and accumulator, thus two effects are expected");

        latch_inner_check(
            block_synchronizer.historical.as_ref(),
            true,
            "should be latched waiting for peers",
        );

        check_need_peer_events(block_hash, mock_reactor.process_effects(effects).await);
    }

    {
        // Inject an empty response from the network, simulating no available peers.
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::NetworkPeers(*block.hash(), vec![]),
        );

        latch_inner_check(
            block_synchronizer.historical.as_ref(),
            true,
            "should still be latched because only one response was received and it \
             did not have what we needed.",
        );

        assert!(effects.is_empty(), "effects should be empty");
    }

    {
        // Inject an empty response from the accumulator, simulating no available peers.
        // as this is the second of two responses, the latch clears. the logic then
        // calls need next again, we still need peers, so we generate the same two effects again.
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::AccumulatedPeers(*block.hash(), None),
        );
        assert!(!effects.is_empty(), "we should still need peers...");

        latch_inner_check(
            block_synchronizer.historical.as_ref(),
            true,
            "we need peers, ask again",
        );

        // We registered no peers, so we still need peers
        need_next_inner_check(
            block_synchronizer.historical.as_mut(),
            &mut rng,
            NeedNext::Peers(block_hash),
            "should need peers",
        );

        check_need_peer_events(block_hash, mock_reactor.process_effects(effects).await);
    }
}

#[tokio::test]
async fn should_not_stall_after_registering_new_era_validator_weights() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let block_hash = *block.hash();
    let era_id = block.header().era_id();

    // Set up a validator matrix.
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix.clone(), Config::default());

    // Set up the synchronizer for the test block such that the next step is getting era validators.
    block_synchronizer.register_block_by_hash(block_hash, true);
    block_synchronizer.register_peers(block_hash, peers.clone());
    block_synchronizer
        .historical
        .as_mut()
        .expect("should have historical builder")
        .register_block_header(block.header().clone(), None)
        .expect("should register block header");

    latch_inner_check(
        block_synchronizer.historical.as_ref(),
        false,
        "initial set up, should not be latched",
    );
    need_next_inner_check(
        block_synchronizer.historical.as_mut(),
        &mut rng,
        NeedNext::EraValidators(era_id),
        "should need era validators for era block is in",
    );

    let effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(
        effects.len(),
        MAX_SIMULTANEOUS_PEERS as usize,
        "need next should have an effect per peer when needing sync leap"
    );
    latch_inner_check(
        block_synchronizer.historical.as_ref(),
        true,
        "after determination that we need validators, should be latched",
    );

    // `need_next` should return no effects while latched.
    assert!(
        block_synchronizer
            .need_next(mock_reactor.effect_builder(), &mut rng)
            .is_empty(),
        "should return no effects while latched"
    );

    // bleed off the event q, checking the expected event kind
    for effect in effects {
        tokio::spawn(async move { effect.await });
        let event = mock_reactor.crank().await;
        match event {
            MockReactorEvent::SyncLeapFetcherRequest(_) => (),
            _ => panic!("unexpected event: {:?}", event),
        };
    }

    // Update the validator matrix to now have an entry for the era of our random block.
    validator_matrix.register_validator_weights(
        era_id,
        iter::once((ALICE_PUBLIC_KEY.clone(), 100.into())).collect(),
    );

    // register validator_matrix
    block_synchronizer
        .historical
        .as_mut()
        .expect("should have historical builder")
        .register_era_validator_weights(&validator_matrix);

    latch_inner_check(
        block_synchronizer.historical.as_ref(),
        false,
        "after registering validators, should not be latched",
    );

    need_next_inner_check(
        block_synchronizer.historical.as_mut(),
        &mut rng,
        NeedNext::FinalitySignatures(block_hash, era_id, validator_matrix.public_keys(&era_id)),
        "should need finality sigs",
    );

    // Ensure the in-flight latch has been released, i.e. that `need_next` returns something.
    let mut effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(
        effects.len(),
        1,
        "need next should produce 1 effect because we currently need exactly 1 signature \
         NOTE: finality signatures are a special case; we currently we fan out 1 peer per signature \
          but do multiple rounds of this against increasingly strict weight thresholds. \
         All other fetchers fan out by asking each of MAX_SIMULTANEOUS_PEERS for the _same_ item."
    );

    tokio::spawn(async move { effects.remove(0).await });
    let event = mock_reactor.crank().await;
    assert_matches!(
        event,
        MockReactorEvent::FinalitySignatureFetcherRequest(FetcherRequest {
            id,
            peer,
            ..
        }) if peers.contains(&peer) && id.block_hash() == block.hash()
    );
}

#[test]
fn duplicate_register_block_not_allowed_if_builder_is_not_failed() {
    let mut rng = TestRng::new();
    let test_env = TestEnv::random(&mut rng);
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for forward sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some()); // we only get global state on historical sync

    // Registering the block again should not be allowed until the sync finishes
    assert!(!block_synchronizer.register_block_by_hash(*block.hash(), false));

    // Trying to register a different block should replace the old one
    let new_block = TestBlockBuilder::new().build(&mut rng);
    assert!(block_synchronizer.register_block_by_hash(*new_block.hash(), false));
    assert_eq!(
        block_synchronizer.forward.unwrap().block_hash(),
        *new_block.hash()
    );
}

#[tokio::test]
async fn historical_sync_gets_peers_form_both_connected_peers_and_accumulator() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    assert!(block_synchronizer.historical.is_some());

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        &mut rng,
        Event::Request(BlockSynchronizerRequest::NeedNext),
    );
    assert_eq!(effects.len(), 2);
    let events = mock_reactor.process_effects(effects).await;

    // The first thing the synchronizer should do is get peers.
    // For the historical flow, the synchronizer will get a random sampling of the connected
    // peers and also ask the accumulator to provide peers from which it has received information
    // for the block that is being synchronized.
    assert_matches!(
        events[0],
        MockReactorEvent::NetworkInfoRequest(NetworkInfoRequest::FullyConnectedPeers {
            count,
            ..
        }) if count == MAX_SIMULTANEOUS_PEERS as usize
    );

    assert_matches!(
        events[1],
        MockReactorEvent::BlockAccumulatorRequest(BlockAccumulatorRequest::GetPeersForBlock {
            block_hash,
            ..
        }) if block_hash == *block.hash()
    )
}

#[tokio::test]
async fn fwd_sync_gets_peers_only_from_accumulator() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for forward sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        &mut rng,
        Event::Request(BlockSynchronizerRequest::NeedNext),
    );
    assert_eq!(effects.len(), 1);
    let events = mock_reactor.process_effects(effects).await;

    // The first thing the synchronizer should do is get peers.
    // For the forward flow, the synchronizer will ask the accumulator to provide peers
    // from which it has received information for the block that is being synchronized.
    assert_matches!(
        events[0],
        MockReactorEvent::BlockAccumulatorRequest(BlockAccumulatorRequest::GetPeersForBlock {
            block_hash,
            ..
        }) if block_hash == *block.hash()
    )
}

#[tokio::test]
async fn sync_starts_with_header_fetch() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let block = test_env.block();
    let peers = test_env.peers();
    let validator_matrix = test_env.gen_validator_matrix();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let events = need_next(
        &mut rng,
        &mock_reactor,
        &mut block_synchronizer,
        MAX_SIMULTANEOUS_PEERS,
    )
    .await;

    // The first thing needed after the synchronizer has peers is
    // to fetch the block header from peers.
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockHeaderFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) if peers.contains(&peer) && id == *block.hash()
        );
    }
}

#[tokio::test]
async fn fwd_sync_is_not_blocked_by_failed_header_fetch_within_latch_interval() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let block = test_env.block();
    let block_hash = *block.hash();
    let peers = test_env.peers();
    let validator_matrix = test_env.gen_validator_matrix();
    let cfg = Config {
        ..Default::default()
    };
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, cfg);

    // Register block for fwd sync
    assert!(
        block_synchronizer.register_block_by_hash(block_hash, false),
        "should register block by hash"
    );
    assert!(
        block_synchronizer.forward.is_some(),
        "should have forward sync"
    );
    block_synchronizer.register_peers(block_hash, peers.clone());

    let events = need_next(
        &mut rng,
        &mock_reactor,
        &mut block_synchronizer,
        MAX_SIMULTANEOUS_PEERS,
    )
    .await;

    let initial_progress = block_synchronizer
        .forward
        .as_ref()
        .expect("should exist")
        .last_progress_time();

    latch_inner_check(
        block_synchronizer.forward.as_ref(),
        true,
        "forward builder should be latched after need next call",
    );

    let mut peers_asked = Vec::new();
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockHeaderFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) if peers.contains(&peer) && id == block_hash => {
                peers_asked.push(peer);
            },
            "should be block header fetch"
        );
    }

    // Simulate fetch errors for the header
    let mut generated_effects = Effects::new();
    for peer in peers_asked {
        latch_inner_check(
            block_synchronizer.forward.as_ref(),
            true,
            &format!("response from peer: {:?}, but should still be latched until after final response received", peer),
        );
        assert!(
            generated_effects.is_empty(),
            "effects should remain empty until last response"
        );
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::BlockHeaderFetched(Err(FetcherError::Absent {
                id: Box::new(*block.hash()),
                peer,
            })),
        );

        // the effects array should be empty while the latch is active
        // once the latch is reset, we should get some effects
        generated_effects.extend(effects);
    }

    need_next_inner_check(
        block_synchronizer.forward.as_mut(),
        &mut rng,
        NeedNext::BlockHeader(block_hash),
        "should need block header",
    );
    assert!(
        !generated_effects.is_empty(),
        "should have gotten effects after the final response tail called into need next"
    );

    latch_inner_check(
        block_synchronizer.forward.as_ref(),
        true,
        "all requests have been responded to, and the last event response should have \
        resulted in a fresh need next being reported and thus a new latch",
    );

    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Syncing(block_hash, _, _) if block_hash == block_hash,
        "should be syncing"
    );

    tokio::time::sleep(Duration::from(cfg.need_next_interval)).await;

    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Syncing(block_hash, _, _) if block_hash == *block.hash()
    );

    let current_progress = block_synchronizer
        .forward
        .as_ref()
        .expect("should exist")
        .last_progress_time();

    assert_eq!(
        initial_progress, current_progress,
        "we have not gotten the record we need, so progress should remain the same"
    )
}

#[tokio::test]
async fn registering_header_successfully_triggers_signatures_fetch_for_weak_finality() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let events = need_next(
        &mut rng,
        &mock_reactor,
        &mut block_synchronizer,
        MAX_SIMULTANEOUS_PEERS,
    )
    .await;

    let mut peers_asked = Vec::new();
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockHeaderFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) if peers.contains(&peer) && id == *block.hash() => {
                peers_asked.push(peer);
            }
        );
    }

    // Simulate successful fetch of the block header
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        &mut rng,
        Event::BlockHeaderFetched(Ok(FetchedData::FromPeer {
            item: Box::new(block.clone().take_header()),
            peer: peers_asked[0],
        })),
    );

    // Check the block acquisition state
    let fwd_builder = block_synchronizer.forward_builder();
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveBlockHeader(header, _) if header.block_hash() == *block.hash()
    );

    // Check if the peer that provided the successful response was promoted
    assert!(fwd_builder.peer_list().is_peer_reliable(&peers_asked[0]));

    // Next the synchronizer should fetch finality signatures to reach weak finality.
    // The number of requests should be limited to the number of peers even if we
    // need to get more signatures to reach weak finality.
    assert_eq!(
        effects.len(),
        min(
            test_env.validator_keys().len(),
            MAX_SIMULTANEOUS_PEERS as usize
        )
    );
    for event in mock_reactor.process_effects(effects).await {
        assert_matches!(
            event,
            MockReactorEvent::FinalitySignatureFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) if peers.contains(&peer) && id.block_hash() == block.hash() && id.era_id() == block.era_id()
        );
    }
}

#[tokio::test]
async fn fwd_more_signatures_are_requested_if_weak_finality_is_not_reached() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveBlockHeader(header, _) if header.block_hash() == *block.hash()
    );

    // Simulate a successful fetch of a single signature
    let signature = FinalitySignature::create(
        *block.hash(),
        block.era_id(),
        validators_secret_keys[0].as_ref(),
    );
    assert!(signature.is_verified().is_ok());
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        &mut rng,
        Event::FinalitySignatureFetched(Ok(FetchedData::FromPeer {
            item: Box::new(signature),
            peer: peers[0],
        })),
    );

    // A single signature isn't enough to reach weak finality.
    // The synchronizer should ask for the remaining signatures.
    // The peer limit should still be in place.
    assert_eq!(
        effects.len(),
        min(
            validators_secret_keys.len() - 1,
            MAX_SIMULTANEOUS_PEERS as usize
        )
    );
    for event in mock_reactor.process_effects(effects).await {
        assert_matches!(
            event,
            MockReactorEvent::FinalitySignatureFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) => {
                assert!(peers.contains(&peer));
                assert_eq!(id.block_hash(), block.hash());
                assert_eq!(id.era_id(), block.era_id());
                assert_ne!(*id.public_key(), PublicKey::from(validators_secret_keys[0].as_ref()));
            }
        );
    }

    // Register finality signatures to reach weak finality
    let mut generated_effects = Effects::new();
    for secret_key in validators_secret_keys
        .iter()
        .skip(1)
        .take(weak_finality_threshold(validators_secret_keys.len()))
    {
        // Register a finality signature
        let signature =
            FinalitySignature::create(*block.hash(), block.era_id(), secret_key.as_ref());
        assert!(signature.is_verified().is_ok());
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::FinalitySignatureFetched(Ok(FetchedData::FromPeer {
                item: Box::new(signature),
                peer: peers[2],
            })),
        );
        generated_effects.extend(effects);
    }

    // Now the block should have weak finality.
    // We are only interested in the last effects generated since as soon as the block has weak
    // finality it should start to fetch the block body.
    let events = mock_reactor
        .process_effects(
            generated_effects
                .into_iter()
                .rev()
                .take(MAX_SIMULTANEOUS_PEERS as usize),
        )
        .await;
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) => {
                assert!(peers.contains(&peer));
                assert_eq!(id, *block.hash());
            }
        );
    }
}

#[tokio::test]
async fn fwd_sync_is_not_blocked_by_failed_signatures_fetch_within_latch_interval() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let expected_block_hash = *block.hash();
    let era_id = block.header().era_id();
    let validator_matrix = test_env.gen_validator_matrix();
    let num_validators = test_env.validator_keys().len() as u8;
    let cfg = Config {
        ..Default::default()
    };
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, cfg);

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(expected_block_hash, false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(expected_block_hash, peers.clone());
    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveBlockHeader(header, _) if header.block_hash() == expected_block_hash
    );

    // Synchronizer should fetch finality signatures
    let events = need_next(
        &mut rng,
        &mock_reactor,
        &mut block_synchronizer,
        min(num_validators, MAX_SIMULTANEOUS_PEERS),
        /* We have num_validators
         * validators so we
         * require the num_validators
         * signatures */
    )
    .await;

    // Check what signatures were requested
    let mut sigs_requested = Vec::new();
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::FinalitySignatureFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) => {
                assert!(peers.contains(&peer));
                assert_eq!(*id.block_hash(), expected_block_hash);
                assert_eq!(id.era_id(), era_id);
                sigs_requested.push((peer, id.public_key().clone()));
            }
        );
    }

    // Simulate failed fetch of finality signatures
    let mut generated_effects = Effects::new();
    for (peer, public_key) in sigs_requested {
        latch_inner_check(
            block_synchronizer.forward.as_ref(),
            true,
            &format!("response from peer: {:?}, but should still be latched until after final response received", peer),
        );
        assert!(
            generated_effects.is_empty(),
            "effects should remain empty until last response"
        );
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::FinalitySignatureFetched(Err(FetcherError::Absent {
                id: Box::new(Box::new(FinalitySignatureId::new(
                    expected_block_hash,
                    era_id,
                    public_key,
                ))),
                peer,
            })),
        );
        // the effects array should be empty while the latch is active
        // once the latch is reset, we should get some effects
        generated_effects.extend(effects);
    }

    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Syncing(block_hash, _, _) if block_hash == expected_block_hash,
        "should be syncing"
    );

    // The effects are empty at this point and the synchronizer is stuck
    assert!(
        !generated_effects.is_empty(),
        "should have gotten effects after the final response tail called into need next"
    );

    latch_inner_check(
        block_synchronizer.forward.as_ref(),
        true,
        "all requests have been responded to, and the last event response should have \
        resulted in a fresh need next being reported and thus a new latch",
    );

    for event in mock_reactor.process_effects(generated_effects).await {
        assert_matches!(
            event,
            MockReactorEvent::FinalitySignatureFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) if peers.contains(&peer) && *id.block_hash() == expected_block_hash && id.era_id() == block.header().era_id()
        );
    }

    // Check if the forward builder is reported as stalled so that the control logic can recover
    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Syncing(block_hash, _, _) if block_hash == expected_block_hash
    );
}

#[tokio::test]
async fn next_action_for_have_weak_finality_is_fetching_block_body() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveWeakFinalitySignatures(header, _) if header.block_hash() == *block.hash()
    );

    // Now the block should have weak finality.
    // Next step is to get the block body.
    let events = need_next(
        &mut rng,
        &mock_reactor,
        &mut block_synchronizer,
        MAX_SIMULTANEOUS_PEERS,
    )
    .await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) => {
                assert!(peers.contains(&peer));
                assert_eq!(id, *block.hash());
            }
        );
    }
}

#[tokio::test]
async fn registering_block_body_transitions_builder_to_have_block_state() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveWeakFinalitySignatures(header, _) if header.block_hash() == *block.hash()
    );

    // Now the block should have weak finality.
    // Next step is to get the block body.
    let events = need_next(
        &mut rng,
        &mock_reactor,
        &mut block_synchronizer,
        MAX_SIMULTANEOUS_PEERS,
    )
    .await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) => {
                assert!(peers.contains(&peer));
                assert_eq!(id, *block.hash());
            }
        );
    }

    block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        &mut rng,
        Event::BlockFetched(Ok(FetchedData::FromPeer {
            item: Box::new(Block::V2(block.clone())),
            peer: peers[0],
        })),
    );

    assert_matches!(
        block_synchronizer.forward_builder().block_acquisition_state(),
        BlockAcquisitionState::HaveBlock(acquired_block, _, _) if acquired_block.hash() == block.hash()
    );
}

#[tokio::test]
async fn fwd_having_block_body_for_block_without_deploys_requires_only_signatures() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );

    assert!(fwd_builder.register_block(block.into(), None).is_ok());

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveBlock(acquired_block, _, _) if acquired_block.hash() == block.hash()
    );

    // Since the block doesn't have any deploys,
    // the next step should be to fetch the finality signatures for strict finality.
    let effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    for event in mock_reactor.process_effects(effects).await {
        assert_matches!(
            event,
            MockReactorEvent::FinalitySignatureFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) if peers.contains(&peer) && id.block_hash() == block.hash() && id.era_id() == block.era_id()
        );
    }
}

#[tokio::test]
async fn fwd_having_block_body_for_block_with_deploys_requires_approvals_hashes() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .random_deploys(1, &mut rng)
            .build(&mut rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );

    assert!(fwd_builder.register_block(block.into(), None).is_ok());

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveBlock(acquired_block, _, _) if acquired_block.hash() == block.hash()
    );

    // Since the block has deploys,
    // the next step should be to fetch the approvals hashes.
    let events = need_next(
        &mut rng,
        &mock_reactor,
        &mut block_synchronizer,
        MAX_SIMULTANEOUS_PEERS,
    )
    .await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::ApprovalsHashesFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) if peers.contains(&peer) && id == *block.hash()
        );
    }
}

#[tokio::test]
async fn fwd_registering_approvals_hashes_triggers_fetch_for_deploys() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let deploys = [Deploy::random(&mut rng)];
    let test_env = TestEnv::random(&mut rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .deploys(deploys.iter())
            .build(&mut rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );

    assert!(fwd_builder.register_block(block.into(), None).is_ok());

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveBlock(acquired_block, _, _) if acquired_block.hash() == block.hash()
    );

    let approvals_hashes = ApprovalsHashes::new(
        block.hash(),
        deploys
            .iter()
            .map(|deploy| deploy.compute_approvals_hash().unwrap())
            .collect(),
        dummy_merkle_proof(),
    );

    // Since the block has approvals hashes,
    // the next step should be to fetch the deploys.
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        &mut rng,
        Event::ApprovalsHashesFetched(Ok(FetchedData::FromPeer {
            item: Box::new(approvals_hashes.clone()),
            peer: peers[0],
        })),
    );
    assert_eq!(effects.len(), MAX_SIMULTANEOUS_PEERS as usize);
    for event in mock_reactor.process_effects(effects).await {
        assert_matches!(
            event,
            MockReactorEvent::DeployFetcherRequest(FetcherRequest {
                id,
                peer,
                ..
            }) => {
                assert!(peers.contains(&peer));
                assert_eq!(id, DeployId::new(
                    *deploys[0].hash(),
                    approvals_hashes.approvals_hashes()[0],
                ));
            }
        );
    }
}

#[tokio::test]
async fn fwd_have_block_body_without_deploys_and_strict_finality_transitions_state_machine() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach strict finality
    register_multiple_signatures(fwd_builder, block, validators_secret_keys.iter());

    assert!(fwd_builder.register_block(block.into(), None).is_ok());

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveBlock(acquired_block, _, _) if acquired_block.hash() == block.hash()
    );

    // Since the block doesn't have any deploys and already has achieved strict finality, we expect
    // it to transition directly to HaveStrictFinality and ask for the next piece of work
    // immediately
    let mut effects = block_synchronizer.need_next(mock_reactor.effect_builder(), &mut rng);
    assert_eq!(effects.len(), 1);

    let fwd_builder = block_synchronizer
        .forward
        .as_ref()
        .expect("Forward builder should have been initialized");
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveStrictFinalitySignatures(acquired_block, ..) if acquired_block.hash() == block.hash()
    );

    // Expect a single NeedNext event
    let events = effects.remove(0).await;
    assert_eq!(events.len(), 1);
    assert_matches!(
        events[0],
        Event::Request(BlockSynchronizerRequest::NeedNext)
    );
}

#[tokio::test]
async fn fwd_have_block_with_strict_finality_requires_creation_of_finalized_block() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register signatures for weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(fwd_builder.register_block(block.into(), None).is_ok());

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveBlock(acquired_block, _, _) if acquired_block.hash() == block.hash()
    );

    // Register the remaining signatures to reach strict finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .skip(weak_finality_threshold(validators_secret_keys.len())),
    );

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveStrictFinalitySignatures(acquired_block, ..) if acquired_block.hash() == block.hash()
    );

    // Block should have strict finality and will require to be executed
    let events = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::MakeBlockExecutableRequest(MakeBlockExecutableRequest {
                block_hash,
                ..
            }) if block_hash == *block.hash()
        );
    }
}

#[tokio::test]
async fn fwd_have_strict_finality_requests_enqueue_when_finalized_block_is_created() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(fwd_builder.register_block(block.into(), None).is_ok());
    // Register the remaining signatures to reach strict finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .skip(weak_finality_threshold(validators_secret_keys.len())),
    );

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveStrictFinalitySignatures(acquired_block, ..) if acquired_block.hash() == block.hash()
    );

    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Syncing(block_hash, _, _) if block_hash == *block.hash()
    );

    // After the FinalizedBlock is created, the block synchronizer will request for it to be
    // enqueued for execution
    let event = Event::MadeFinalizedBlock {
        block_hash: *block.hash(),
        result: Some((block.clone().into(), Vec::new())),
    };
    let effects = block_synchronizer.handle_event(mock_reactor.effect_builder(), &mut rng, event);
    assert_eq!(effects.len(), 1);
    let events = mock_reactor.process_effects(effects).await;

    // Check the block acquisition state
    let fwd_builder = block_synchronizer
        .forward
        .as_ref()
        .expect("Forward builder should have been initialized");
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveFinalizedBlock(actual_block, _, _, _) if *actual_block.hash() == *block.hash()
    );

    // This is the first of two events created when `EffectBuilder::enqueue_block_for_execution` is
    // called.
    assert_matches!(
        &events[0],
        MockReactorEvent::StorageRequest(
            StorageRequest::GetKeyBlockHeightForActivationPoint { .. }
        )
    );

    // Progress is syncing until we get a confirmation that the block was enqueued for execution
    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Syncing(block_hash, _, _) if block_hash == *block.hash()
    );
}

#[tokio::test]
async fn fwd_builder_status_is_executing_when_block_is_enqueued_for_execution() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(fwd_builder.register_block(block.into(), None).is_ok());
    // Register the remaining signatures to reach strict finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .skip(weak_finality_threshold(validators_secret_keys.len())),
    );

    // Check the block acquisition state
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveStrictFinalitySignatures(acquired_block, ..) if acquired_block.hash() == block.hash()
    );

    // Register finalized block
    fwd_builder.register_made_finalized_block(block.clone().into(), Vec::new());
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::HaveFinalizedBlock(actual_block, _, _, _) if *actual_block.hash() == *block.hash()
    );

    // Simulate that enqueuing the block for execution was successful
    let event = Event::MarkBlockExecutionEnqueued(*block.hash());

    // There is nothing for the synchronizer to do at this point.
    // It will wait for the block to be executed
    let effects = block_synchronizer.handle_event(mock_reactor.effect_builder(), &mut rng, event);
    assert_eq!(effects.len(), 0);

    // Progress should now indicate that the block is executing
    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Executing(block_hash, _, _) if block_hash == *block.hash()
    );
}

#[tokio::test]
async fn fwd_sync_is_finished_when_block_is_marked_as_executed() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(fwd_builder.register_block(block.into(), None).is_ok());
    // Register the remaining signatures to reach strict finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .skip(weak_finality_threshold(validators_secret_keys.len())),
    );

    // Register finalized block
    fwd_builder.register_made_finalized_block(block.clone().into(), Vec::new());
    fwd_builder.register_block_execution_enqueued();

    // Progress should now indicate that the block is executing
    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Executing(block_hash, _, _) if block_hash == *block.hash()
    );

    // Simulate a MarkBlockExecuted event
    let event = Event::MarkBlockExecuted(*block.hash());

    // There is nothing for the synchronizer to do at this point, the sync is finished.
    let effects = block_synchronizer.handle_event(mock_reactor.effect_builder(), &mut rng, event);
    assert_eq!(effects.len(), 0);

    // Progress should now indicate that the block is executing
    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Synced(block_hash, _, _) if block_hash == *block.hash()
    );
}

#[tokio::test]
async fn historical_sync_announces_meta_block() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    assert!(block_synchronizer.historical.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let historical_builder = block_synchronizer
        .historical
        .as_mut()
        .expect("Historical builder should have been initialized");
    assert!(historical_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);

    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(historical_builder
        .register_block(block.into(), None)
        .is_ok());
    // Register the remaining signatures to reach strict finality
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .skip(weak_finality_threshold(validators_secret_keys.len())),
    );

    // Set the builder state to `HaveStrictFinalitySignatures`
    match historical_builder.block_acquisition_state() {
        BlockAcquisitionState::HaveBlock(state_block, state_signatures, _) => historical_builder
            .set_block_acquisition_state(BlockAcquisitionState::HaveStrictFinalitySignatures(
                state_block.clone(),
                state_signatures.clone(),
            )),
        other => panic!("Unexpected state: {:?}", other),
    }
    // Make sure the historical builder is syncing
    assert_matches!(
        block_synchronizer.historical_progress(),
        BlockSynchronizerProgress::Syncing(block_hash, _, _) if block_hash == *block.hash()
    );

    // Simulate a MarkBlockCompleted event
    let event = Event::MarkBlockCompleted {
        block_hash: *block.hash(),
        is_new: true,
    };
    // Put it through to the synchronizer
    let effects = block_synchronizer.handle_event(mock_reactor.effect_builder(), &mut rng, event);
    assert_eq!(effects.len(), 1);
    let mut events = mock_reactor.process_effects(effects).await;

    // We should have a request to get the execution results
    match events.pop().unwrap() {
        MockReactorEvent::StorageRequest(StorageRequest::GetExecutionResults {
            block_hash: actual_block_hash,
            responder,
        }) => {
            assert_eq!(actual_block_hash, *block.hash());
            // We'll just send empty execution results for this case.
            responder.respond(Some(vec![])).await;
        }
        other => panic!("Unexpected event: {:?}", other),
    }
    // Crank one more time because the meta block event is chained onto the
    // execution results fetching
    let event = mock_reactor.crank().await;
    match event {
        MockReactorEvent::MetaBlockAnnouncement(MetaBlockAnnouncement(mut meta_block)) => {
            assert_eq!(meta_block.hash(), *block.hash());
            // The deploy buffer is supposed to get notified
            assert!(meta_block
                .mut_state()
                .register_as_sent_to_deploy_buffer()
                .was_updated());
        }
        other => panic!("Unexpected event: {:?}", other),
    }
    // The historical sync for this block should now be complete
    assert_matches!(
        block_synchronizer.historical_progress(),
        BlockSynchronizerProgress::Synced(block_hash, _, _) if block_hash == *block.hash()
    );
}

#[test]
fn builders_are_purged_when_requested() {
    let mut rng = TestRng::new();
    let test_env = TestEnv::random(&mut rng);
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for forward sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));

    // Registering block for historical sync
    assert!(block_synchronizer
        .register_block_by_hash(*TestBlockBuilder::new().build(&mut rng).hash(), true));

    assert!(block_synchronizer.forward.is_some());
    assert!(block_synchronizer.historical.is_some());

    block_synchronizer.purge_historical();
    assert!(block_synchronizer.forward.is_some());
    assert!(block_synchronizer.historical.is_none());

    assert!(block_synchronizer
        .register_block_by_hash(*TestBlockBuilder::new().build(&mut rng).hash(), true));
    assert!(block_synchronizer.forward.is_some());
    assert!(block_synchronizer.historical.is_some());

    block_synchronizer.purge_forward();
    assert!(block_synchronizer.forward.is_none());
    assert!(block_synchronizer.historical.is_some());

    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    assert!(block_synchronizer.historical.is_some());

    block_synchronizer.purge();
    assert!(block_synchronizer.forward.is_none());
    assert!(block_synchronizer.historical.is_none());
}

#[tokio::test]
async fn synchronizer_halts_if_block_cannot_be_made_executable() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(&mut rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let fwd_builder = block_synchronizer
        .forward
        .as_mut()
        .expect("Forward builder should have been initialized");
    assert!(fwd_builder
        .register_block_header(block.clone().take_header(), None)
        .is_ok());
    fwd_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);
    // Register finality signatures to reach weak finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(fwd_builder.register_block(block.into(), None).is_ok());
    // Register the remaining signatures to reach strict finality
    register_multiple_signatures(
        fwd_builder,
        block,
        validators_secret_keys
            .iter()
            .skip(weak_finality_threshold(validators_secret_keys.len())),
    );

    // Block should have strict finality and will require to be executed
    let events = need_next(&mut rng, &mock_reactor, &mut block_synchronizer, 1).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::MakeBlockExecutableRequest(MakeBlockExecutableRequest {
                block_hash,
                ..
            }) if block_hash == *block.hash()
        );
    }

    // Simulate an error (the block couldn't be converted for execution).
    // This can happen if the synchronizer didn't fetch the right approvals hashes.
    // Don't expect to progress any further here. The control logic should
    // leap and backfill this block during a historical sync.
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        &mut rng,
        Event::MadeFinalizedBlock {
            block_hash: *block.hash(),
            result: None,
        },
    );
    assert_eq!(effects.len(), 0);

    // Check the block acquisition state
    let fwd_builder = block_synchronizer
        .forward
        .as_ref()
        .expect("Forward builder should have been initialized");
    assert_matches!(
        fwd_builder.block_acquisition_state(),
        BlockAcquisitionState::Failed(block_hash, _) if block_hash == block.hash()
    );

    // Progress should now indicate that the block is syncing
    assert_matches!(
        block_synchronizer.forward_progress(),
        BlockSynchronizerProgress::Syncing(block_hash, _, _) if block_hash == *block.hash()
    );
}

fn historical_state(block_synchronizer: &BlockSynchronizer) -> &BlockAcquisitionState {
    block_synchronizer
        .historical
        .as_ref()
        .unwrap()
        .block_acquisition_state()
}

/// When there is no deploy, the state goes from `HaveGlobalState` to `HaveStrictFinalitySignature`
/// directly, skipping `HaveAllExecutionResults`, `HaveApprovalsHashes` and `HaveAllDeploys`.
#[tokio::test]
async fn historical_sync_skips_exec_results_and_deploys_if_block_empty() {
    let rng = &mut TestRng::new();
    let mock_reactor = MockReactor::new();
    let test_env = TestEnv::random(rng);
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(rng, validator_matrix, Default::default())
            .with_legacy_finality(LegacyRequiredFinality::Strict);

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    assert!(block_synchronizer.forward.is_none());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    // Skip steps HaveBlockHeader, HaveWeakFinalitySignature, HaveBlock

    let historical_builder = block_synchronizer
        .historical
        .as_mut()
        .expect("Historical builder should have been initialized");
    historical_builder
        .register_block_header(block.clone().take_header(), None)
        .expect("header registration works");
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(historical_builder
        .register_block(block.into(), None)
        .is_ok());

    let events = need_next(rng, &mock_reactor, &mut block_synchronizer, 1).await;

    let request = match events.try_one() {
        Some(MockReactorEvent::SyncGlobalStateRequest(
            request @ SyncGlobalStateRequest {
                block_hash,
                state_root_hash,
                ..
            },
        )) if block_hash == *block.hash() && &state_root_hash == block.state_root_hash() => request,
        _ => panic!("there should be a unique event of type SyncGlobalStateRequest"),
    };

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynchronizer(global_state_synchronizer::Event::Request(request)),
    );

    // ----- HaveBlock -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveBlock { .. }
    );

    // Those effects are handled directly and not through the reactor:
    let events = effects
        .try_one()
        .expect("there should be only one effect")
        .await;
    assert_matches!(
        events.try_one(),
        Some(Event::GlobalStateSynchronizer(
            GlobalStateSynchronizerEvent::GetPeers(_)
        ))
    );

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynced {
            block_hash: *block.hash(),
            result: Ok(GlobalStateSynchronizerResponse::new(
                super::global_state_synchronizer::RootHash::new(*block.state_root_hash()),
                vec![],
            )),
        },
    );

    // ----- HaveGlobalState -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );
    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(event, MockReactorEvent::FinalitySignatureFetcherRequest(..));
    }
}

#[tokio::test]
async fn historical_sync_no_legacy_block() {
    let rng = &mut TestRng::new();
    let mock_reactor = MockReactor::new();
    let deploy = Deploy::random(rng);
    let test_env = TestEnv::random(rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .deploys(std::iter::once(&deploy))
            .build(rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(rng, validator_matrix, Default::default())
            .with_legacy_finality(LegacyRequiredFinality::Strict);

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    assert!(block_synchronizer.forward.is_none());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let historical_builder = block_synchronizer
        .historical
        .as_mut()
        .expect("Historical builder should have been initialized");
    historical_builder
        .register_block_header(block.clone().take_header(), None)
        .expect("header registration works");
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(historical_builder
        .register_block(block.into(), None)
        .is_ok());

    let events = need_next(rng, &mock_reactor, &mut block_synchronizer, 1).await;

    let request = match events.try_one() {
        Some(MockReactorEvent::SyncGlobalStateRequest(
            request @ SyncGlobalStateRequest {
                block_hash,
                state_root_hash,
                ..
            },
        )) if block_hash == *block.hash() && &state_root_hash == block.state_root_hash() => request,
        _ => panic!("there should be a unique event of type SyncGlobalStateRequest"),
    };

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynchronizer(global_state_synchronizer::Event::Request(request)),
    );

    // Those effects are handled directly and not through the reactor:
    let events = effects.one().await;
    assert_matches!(
        events.try_one(),
        Some(Event::GlobalStateSynchronizer(
            GlobalStateSynchronizerEvent::GetPeers(_)
        ))
    );

    // ----- HaveBlock -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveBlock { .. }
    );

    // Let's not test the detail of the global synchronization event,
    // since it is already tested in its unit tests.

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynced {
            block_hash: *block.hash(),
            result: Ok(GlobalStateSynchronizerResponse::new(
                super::global_state_synchronizer::RootHash::new(*block.state_root_hash()),
                vec![],
            )),
        },
    );

    // ----- HaveGlobalState -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    let events = mock_reactor.process_effects(effects).await;

    match events.try_one() {
        Some(MockReactorEvent::ContractRuntimeRequest(
            ContractRuntimeRequest::GetExecutionResultsChecksum {
                state_root_hash,
                responder,
            },
        )) => responder.respond(Ok(Some(state_root_hash))).await,
        other => panic!("Event should be of type `ContractRuntimeRequest(ContractRuntimeRequest::GetExecutionResultsChecksum) but it is {:?}", other),
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GotExecutionResultsChecksum {
            block_hash: *block.hash(),
            result: Ok(Some(Digest::SENTINEL_NONE)),
        },
    );
    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockExecutionResultsOrChunkFetcherRequest(FetcherRequest { .. })
        );
    }

    let execution_results = BlockExecutionResultsOrChunk::new_mock_value(rng, *block.hash());
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsFetched {
            block_hash: *block.hash(),
            result: Ok(FetchedData::from_storage(Box::new(execution_results))),
        },
    );

    let mut events = mock_reactor.process_effects(effects).await;

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    assert_matches!(
        events.remove(0),
        MockReactorEvent::StorageRequest(StorageRequest::PutExecutionResults { .. })
    );
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::ApprovalsHashesFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsStored(*block.hash()),
    );
    // ----- HaveAllExecutionResults -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveAllExecutionResults(_, _, _, checksum) if checksum.is_checkable()
    );

    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::ApprovalsHashesFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ApprovalsHashesFetched(Ok(FetchedData::from_storage(Box::new(
            ApprovalsHashes::new(
                block.hash(),
                vec![deploy.compute_approvals_hash().unwrap()],
                dummy_merkle_proof(),
            ),
        )))),
    );
    // ----- HaveApprovalsHashes -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
    );

    let events = mock_reactor.process_effects(effects).await;
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::DeployFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::DeployFetched {
            block_hash: *block.hash(),
            result: Either::Right(Ok(FetchedData::from_storage(Box::new(deploy)))),
        },
    );
    // ----- HaveAllDeploys -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveAllDeploys(_, _)
    );

    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(event, MockReactorEvent::FinalitySignatureFetcherRequest(_));
    }

    // Then we get back to the strict finality signature part, which is already tested.
}

#[tokio::test]
async fn historical_sync_legacy_block_strict_finality() {
    let rng = &mut TestRng::new();
    let mock_reactor = MockReactor::new();
    let deploy = Deploy::random(rng);
    let test_env = TestEnv::random(rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .deploys(std::iter::once(&deploy))
            .build(rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(rng, validator_matrix, Default::default())
            .with_legacy_finality(LegacyRequiredFinality::Strict);

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    assert!(block_synchronizer.forward.is_none());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let historical_builder = block_synchronizer
        .historical
        .as_mut()
        .expect("Historical builder should have been initialized");
    historical_builder
        .register_block_header(block.clone().take_header(), None)
        .expect("header registration works");
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(historical_builder
        .register_block(block.into(), None)
        .is_ok());

    let events = need_next(rng, &mock_reactor, &mut block_synchronizer, 1).await;

    let request = match events.try_one() {
        Some(MockReactorEvent::SyncGlobalStateRequest(
            request @ SyncGlobalStateRequest {
                block_hash,
                state_root_hash,
                ..
            },
        )) if block_hash == *block.hash() && &state_root_hash == block.state_root_hash() => request,
        _ => panic!("there should be a unique event of type SyncGlobalStateRequest"),
    };

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynchronizer(global_state_synchronizer::Event::Request(request)),
    );

    // Those effects are handled directly and not through the reactor:
    let events = effects.one().await;
    assert_matches!(
        events.try_one(),
        Some(Event::GlobalStateSynchronizer(
            GlobalStateSynchronizerEvent::GetPeers(_)
        ))
    );

    // ----- HaveBlock -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveBlock { .. }
    );

    // Let's not test the detail of the global synchronization event,
    // since it is already tested in its unit tests.

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynced {
            block_hash: *block.hash(),
            result: Ok(GlobalStateSynchronizerResponse::new(
                super::global_state_synchronizer::RootHash::new(*block.state_root_hash()),
                vec![],
            )),
        },
    );

    // ----- HaveGlobalState -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    let events = mock_reactor.process_effects(effects).await;

    match events.try_one() {
        Some(MockReactorEvent::ContractRuntimeRequest(
            ContractRuntimeRequest::GetExecutionResultsChecksum {
                state_root_hash,
                responder,
            },
        )) => responder.respond(Ok(Some(state_root_hash))).await,
        other => panic!("Event should be of type `ContractRuntimeRequest(ContractRuntimeRequest::GetExecutionResultsChecksum) but it is {:?}", other),
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GotExecutionResultsChecksum {
            block_hash: *block.hash(),
            result: Ok(None), // No checksum because we want to test a legacy block
        },
    );
    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockExecutionResultsOrChunkFetcherRequest(FetcherRequest { .. })
        );
    }

    let execution_results = BlockExecutionResultsOrChunk::new_mock_value(rng, *block.hash());
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsFetched {
            block_hash: *block.hash(),
            result: Ok(FetchedData::from_storage(Box::new(execution_results))),
        },
    );

    let mut events = mock_reactor.process_effects(effects).await;

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    assert_matches!(
        events.remove(0),
        MockReactorEvent::StorageRequest(StorageRequest::PutExecutionResults { .. })
    );
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::ApprovalsHashesFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsStored(*block.hash()),
    );
    // ----- HaveAllExecutionResults -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveAllExecutionResults(_, _, _, checksum)
            if checksum.is_checkable() == false
    );

    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::LegacyDeployFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::DeployFetched {
            block_hash: *block.hash(),
            result: Either::Left(Ok(FetchedData::from_storage(Box::new(deploy.into())))),
        },
    );
    // ----- HaveAllDeploys -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveAllDeploys(_, _)
    );

    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(event, MockReactorEvent::FinalitySignatureFetcherRequest(_));
    }

    // Then we get back to the strict finality signature part, which is already tested.
}

#[tokio::test]
async fn historical_sync_legacy_block_weak_finality() {
    let rng = &mut TestRng::new();
    let mock_reactor = MockReactor::new();
    let deploy = Deploy::random(rng);
    let test_env = TestEnv::random(rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .deploys(std::iter::once(&deploy))
            .build(rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(rng, validator_matrix, Default::default())
            .with_legacy_finality(LegacyRequiredFinality::Weak);

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    assert!(block_synchronizer.forward.is_none());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let historical_builder = block_synchronizer
        .historical
        .as_mut()
        .expect("Historical builder should have been initialized");
    historical_builder
        .register_block_header(block.clone().take_header(), None)
        .expect("header registration works");
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(historical_builder
        .register_block(block.into(), None)
        .is_ok());

    let events = need_next(rng, &mock_reactor, &mut block_synchronizer, 1).await;

    let request = match events.try_one() {
        Some(MockReactorEvent::SyncGlobalStateRequest(
            request @ SyncGlobalStateRequest {
                block_hash,
                state_root_hash,
                ..
            },
        )) if block_hash == *block.hash() && &state_root_hash == block.state_root_hash() => request,
        _ => panic!("there should be a unique event of type SyncGlobalStateRequest"),
    };

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynchronizer(global_state_synchronizer::Event::Request(request)),
    );

    // Those effects are handled directly and not through the reactor:
    let events = effects.one().await;
    assert_matches!(
        events.try_one(),
        Some(Event::GlobalStateSynchronizer(
            GlobalStateSynchronizerEvent::GetPeers(_)
        ))
    );

    // ----- HaveBlock -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveBlock { .. }
    );

    // Let's not test the detail of the global synchronization event,
    // since it is already tested in its unit tests.

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynced {
            block_hash: *block.hash(),
            result: Ok(GlobalStateSynchronizerResponse::new(
                super::global_state_synchronizer::RootHash::new(*block.state_root_hash()),
                vec![],
            )),
        },
    );

    // ----- HaveGlobalState -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    let events = mock_reactor.process_effects(effects).await;

    match events.try_one() {
        Some(MockReactorEvent::ContractRuntimeRequest(
            ContractRuntimeRequest::GetExecutionResultsChecksum {
                state_root_hash,
                responder,
            },
        )) => responder.respond(Ok(Some(state_root_hash))).await,
        other => panic!("Event should be of type `ContractRuntimeRequest(ContractRuntimeRequest::GetExecutionResultsChecksum) but it is {:?}", other),
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GotExecutionResultsChecksum {
            block_hash: *block.hash(),
            result: Ok(None), // No checksum because we want to test a legacy block
        },
    );
    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockExecutionResultsOrChunkFetcherRequest(FetcherRequest { .. })
        );
    }

    let execution_results = BlockExecutionResultsOrChunk::new_mock_value(rng, *block.hash());
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsFetched {
            block_hash: *block.hash(),
            result: Ok(FetchedData::from_storage(Box::new(execution_results))),
        },
    );

    let mut events = mock_reactor.process_effects(effects).await;

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    assert_matches!(
        events.remove(0),
        MockReactorEvent::StorageRequest(StorageRequest::PutExecutionResults { .. })
    );
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::ApprovalsHashesFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsStored(*block.hash()),
    );
    // ----- HaveAllExecutionResults -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveAllExecutionResults(_, _, _, checksum)
            if checksum.is_checkable() == false
    );

    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::LegacyDeployFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::DeployFetched {
            block_hash: *block.hash(),
            result: Either::Left(Ok(FetchedData::from_storage(Box::new(deploy.into())))),
        },
    );

    // ----- HaveStrictFinalitySignatures -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
    );

    let events = effects.one().await;

    let event = match events.try_one() {
        Some(event @ Event::Request(BlockSynchronizerRequest::NeedNext)) => event,
        _ => panic!("Expected a NeedNext request here"),
    };

    let effects = block_synchronizer.handle_event(mock_reactor.effect_builder(), rng, event);

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
    );

    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(event, MockReactorEvent::MarkBlockCompletedRequest(_));
    }
}

#[tokio::test]
async fn historical_sync_legacy_block_any_finality() {
    let rng = &mut TestRng::new();
    let mock_reactor = MockReactor::new();
    let deploy = Deploy::random(rng);
    let test_env = TestEnv::random(rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .deploys(std::iter::once(&deploy))
            .build(rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(rng, validator_matrix, Default::default())
            .with_legacy_finality(LegacyRequiredFinality::Any);

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    assert!(block_synchronizer.forward.is_none());
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let historical_builder = block_synchronizer
        .historical
        .as_mut()
        .expect("Historical builder should have been initialized");
    historical_builder
        .register_block_header(block.clone().take_header(), None)
        .expect("header registration works");
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys.iter().take(1),
    );
    assert!(historical_builder
        .register_block(block.into(), None)
        .is_ok());

    let events = need_next(rng, &mock_reactor, &mut block_synchronizer, 1).await;

    let request = match events.try_one() {
        Some(MockReactorEvent::SyncGlobalStateRequest(
            request @ SyncGlobalStateRequest {
                block_hash,
                state_root_hash,
                ..
            },
        )) if block_hash == *block.hash() && &state_root_hash == block.state_root_hash() => request,
        _ => panic!("there should be a unique event of type SyncGlobalStateRequest"),
    };

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynchronizer(global_state_synchronizer::Event::Request(request)),
    );

    // Those effects are handled directly and not through the reactor:
    let events = effects.one().await;
    assert_matches!(
        events.try_one(),
        Some(Event::GlobalStateSynchronizer(
            GlobalStateSynchronizerEvent::GetPeers(_)
        ))
    );

    // ----- HaveBlock -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveBlock { .. }
    );

    // Let's not test the detail of the global synchronization event,
    // since it is already tested in its unit tests.

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynced {
            block_hash: *block.hash(),
            result: Ok(GlobalStateSynchronizerResponse::new(
                super::global_state_synchronizer::RootHash::new(*block.state_root_hash()),
                vec![],
            )),
        },
    );

    // ----- HaveGlobalState -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    let events = mock_reactor.process_effects(effects).await;

    match events.try_one() {
        Some(MockReactorEvent::ContractRuntimeRequest(
            ContractRuntimeRequest::GetExecutionResultsChecksum {
                state_root_hash,
                responder,
            },
        )) => responder.respond(Ok(Some(state_root_hash))).await,
        other => panic!("Event should be of type `ContractRuntimeRequest(ContractRuntimeRequest::GetExecutionResultsChecksum) but it is {:?}", other),
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GotExecutionResultsChecksum {
            block_hash: *block.hash(),
            result: Ok(None), // No checksum because we want to test a legacy block
        },
    );
    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::BlockExecutionResultsOrChunkFetcherRequest(FetcherRequest { .. })
        );
    }

    let execution_results = BlockExecutionResultsOrChunk::new_mock_value(rng, *block.hash());
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsFetched {
            block_hash: *block.hash(),
            result: Ok(FetchedData::from_storage(Box::new(execution_results))),
        },
    );

    let mut events = mock_reactor.process_effects(effects).await;

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    assert_matches!(
        events.remove(0),
        MockReactorEvent::StorageRequest(StorageRequest::PutExecutionResults { .. })
    );
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::ApprovalsHashesFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsStored(*block.hash()),
    );
    // ----- HaveAllExecutionResults -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveAllExecutionResults(_, _, _, checksum)
            if checksum.is_checkable() == false
    );

    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::LegacyDeployFetcherRequest(FetcherRequest { .. })
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::DeployFetched {
            block_hash: *block.hash(),
            result: Either::Left(Ok(FetchedData::from_storage(Box::new(deploy.into())))),
        },
    );

    // ----- HaveStrictFinalitySignatures -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
    );

    let events = effects.one().await;

    let event = match events.try_one() {
        Some(event @ Event::Request(BlockSynchronizerRequest::NeedNext)) => event,
        _ => panic!("Expected a NeedNext request here"),
    };

    let effects = block_synchronizer.handle_event(mock_reactor.effect_builder(), rng, event);

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveStrictFinalitySignatures(_, _)
    );

    let events = mock_reactor.process_effects(effects).await;

    for event in events {
        assert_matches!(event, MockReactorEvent::MarkBlockCompletedRequest(_));
    }
}

#[tokio::test]
async fn fwd_sync_latch_should_not_decrement_for_old_responses() {
    let mut rng = TestRng::new();
    let mock_reactor = MockReactor::new();
    let deploys = [Deploy::random(&mut rng)];
    let test_env = TestEnv::random(&mut rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .deploys(deploys.iter())
            .build(&mut rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(&mut rng, validator_matrix, Config::default());

    // Register block for fwd sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), false));
    assert!(block_synchronizer.forward.is_some());

    // Start syncing.
    {
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::Request(BlockSynchronizerRequest::NeedNext),
        );
        assert_eq!(effects.len(), 1);

        // First, the synchronizer should get peers.
        let events = mock_reactor.process_effects(effects).await;
        assert_matches!(
            events[0],
            MockReactorEvent::BlockAccumulatorRequest(BlockAccumulatorRequest::GetPeersForBlock {
                block_hash,
                ..
            }) if block_hash == *block.hash()
        );

        latch_inner_check(
            block_synchronizer.forward.as_ref(),
            true,
            "should be latched waiting for peers",
        );
    }

    // Register peers. This would make the synchronizer ask for the block header.
    {
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::AccumulatedPeers(*block.hash(), Some(peers.clone())),
        );
        let events = mock_reactor.process_effects(effects).await;

        let mut peers_asked = Vec::new();
        for event in events {
            assert_matches!(
                event,
                MockReactorEvent::BlockHeaderFetcherRequest(FetcherRequest {
                    id,
                    peer,
                    ..
                }) if peers.contains(&peer) && id == *block.hash() => {
                    peers_asked.push(peer);
                }
            );
        }

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            format!(
                "Latch count should be {} since no block header was received.",
                MAX_SIMULTANEOUS_PEERS
            )
            .as_str(),
        );

        // Simulate successful fetch of the block header.
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::BlockHeaderFetched(Ok(FetchedData::FromPeer {
                item: Box::new(block.clone().take_header()),
                peer: peers_asked[0],
            })),
        );
        let events = mock_reactor.process_effects(effects).await;

        let expected_latch_count = events.len() as u8; // number of finality sig fetches.

        // Check what signatures were requested
        let mut sigs_requested = Vec::new();
        for event in events {
            assert_matches!(
                event,
                MockReactorEvent::FinalitySignatureFetcherRequest(FetcherRequest {
                    id,
                    peer,
                    ..
                }) => {
                    assert_eq!(id.block_hash(), block.hash());
                    assert_eq!(id.era_id(), block.header().era_id());
                    sigs_requested.push((peer, id.public_key().clone()));
                }
            );
        }

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            expected_latch_count,
            format!(
                "Latch count should be {} since no finality sigs were received.",
                expected_latch_count
            )
            .as_str(),
        );

        // Receive a late response with the block header.
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::BlockHeaderFetched(Ok(FetchedData::FromPeer {
                item: Box::new(block.clone().take_header()),
                peer: peers_asked[1],
            })),
        );

        assert_eq!(effects.len(), 0);
        latch_count_check(
            block_synchronizer.forward.as_ref(),
            expected_latch_count,
            format!(
                "Latch count should be {} since no finality sigs were received.",
                expected_latch_count
            )
            .as_str(),
        );
    }

    // Register finality sigs. This would make the synchronizer switch to have weak finality and
    // continue asking for the block body.
    {
        let mut generated_effects = Effects::new();
        for secret_key in validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len()))
        {
            // Register a finality signature
            let signature = FinalitySignature::create(
                *block.hash(),
                block.header().era_id(),
                secret_key.as_ref(),
            );
            assert!(signature.is_verified().is_ok());
            let effects = block_synchronizer.handle_event(
                mock_reactor.effect_builder(),
                &mut rng,
                Event::FinalitySignatureFetched(Ok(FetchedData::FromPeer {
                    item: Box::new(signature),
                    peer: peers[2],
                })),
            );
            generated_effects.extend(effects);
        }

        let events = mock_reactor
            .process_effects(
                generated_effects
                    .into_iter()
                    .rev()
                    .take(MAX_SIMULTANEOUS_PEERS as usize),
            )
            .await;

        for event in events {
            assert_matches!(
                event,
                MockReactorEvent::BlockFetcherRequest(FetcherRequest {
                    id,
                    peer,
                    ..
                }) => {
                    assert!(peers.contains(&peer));
                    assert_eq!(id, *block.hash());
                }
            );
        }

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            format!(
                "Latch count should be {} since no block was received.",
                MAX_SIMULTANEOUS_PEERS
            )
            .as_str(),
        );

        // Receive some more finality signatures to check if the latch decrements.
        let mut generated_effects = Effects::new();
        for secret_key in validators_secret_keys
            .iter()
            .skip(weak_finality_threshold(validators_secret_keys.len()))
            .take(2)
        {
            // Register a finality signature
            let signature = FinalitySignature::create(
                *block.hash(),
                block.header().era_id(),
                secret_key.as_ref(),
            );
            assert!(signature.is_verified().is_ok());
            let effects = block_synchronizer.handle_event(
                mock_reactor.effect_builder(),
                &mut rng,
                Event::FinalitySignatureFetched(Ok(FetchedData::FromPeer {
                    item: Box::new(signature),
                    peer: peers[2],
                })),
            );
            generated_effects.extend(effects);
        }

        assert_eq!(generated_effects.len(), 0);

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            format!(
                "Latch count should be {} since no block was received.",
                MAX_SIMULTANEOUS_PEERS
            )
            .as_str(),
        );
    }

    // Register a block response. This would make the synchronizer switch to HaveBlock and continue
    // asking for the approvals hashes.
    {
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::BlockFetched(Ok(FetchedData::FromPeer {
                item: Box::new(block.clone().into()),
                peer: peers[0],
            })),
        );
        let events = mock_reactor.process_effects(effects).await;

        for event in events {
            assert_matches!(
                event,
                MockReactorEvent::ApprovalsHashesFetcherRequest(FetcherRequest {
                    id,
                    peer,
                    ..
                }) if peers.contains(&peer) && id == *block.hash()
            );
        }

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            format!(
                "Latch count should be {} since no approval hashes were received.",
                MAX_SIMULTANEOUS_PEERS
            )
            .as_str(),
        );

        // Receive another response with the block. This is the second response out of the 5 we sent
        // out earlier.
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::BlockFetched(Ok(FetchedData::FromPeer {
                item: Box::new(block.clone().into()),
                peer: peers[1],
            })),
        );
        assert_eq!(effects.len(), 0);

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            format!(
                "Latch count should be {} since no approval hashes were received.",
                MAX_SIMULTANEOUS_PEERS
            )
            .as_str(),
        );
    }

    // Register approvals hashes. This would make the synchronizer switch to HaveApprovalsHashes and
    // continue asking for the deploys.
    {
        let approvals_hashes = ApprovalsHashes::new(
            block.hash(),
            deploys
                .iter()
                .map(|deploy| deploy.compute_approvals_hash().unwrap())
                .collect(),
            dummy_merkle_proof(),
        );
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::ApprovalsHashesFetched(Ok(FetchedData::FromPeer {
                item: Box::new(approvals_hashes.clone()),
                peer: peers[0],
            })),
        );
        assert_eq!(effects.len(), MAX_SIMULTANEOUS_PEERS as usize);
        for event in mock_reactor.process_effects(effects).await {
            assert_matches!(
                event,
                MockReactorEvent::DeployFetcherRequest(FetcherRequest {
                    id,
                    peer,
                    ..
                }) => {
                    assert!(peers.contains(&peer));
                    assert_eq!(id, DeployId::new(
                        *deploys[0].hash(),
                        approvals_hashes.approvals_hashes()[0],
                    ));
                }
            );
        }

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            format!(
                "Latch count should be {} since no deploys were received.",
                MAX_SIMULTANEOUS_PEERS
            )
            .as_str(),
        );

        // Receive a late response with the approvals hashes.
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::ApprovalsHashesFetched(Ok(FetchedData::FromPeer {
                item: Box::new(approvals_hashes.clone()),
                peer: peers[1],
            })),
        );

        assert_eq!(effects.len(), 0);
        latch_count_check(
            block_synchronizer.forward.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            format!(
                "Latch count should be {} since no deploys were received.",
                MAX_SIMULTANEOUS_PEERS
            )
            .as_str(),
        );
    }

    // Receive a deploy. This would make the synchonizer switch to HaveAllDeploys and continue
    // asking for more finality signatures in order to reach strict finality.
    {
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::DeployFetched {
                block_hash: *block.hash(),
                result: Either::Right(Ok(FetchedData::from_storage(Box::new(deploys[0].clone())))),
            },
        );
        let events = mock_reactor.process_effects(effects).await;
        let expected_latch_count = events.len() as u8;

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            expected_latch_count,
            format!(
                "Latch count should be {} since no new signatures were received.",
                expected_latch_count
            )
            .as_str(),
        );

        // Since it's the single deploy in the block, the next step is to get the rest of the
        // finality signatures to get strict finality.
        for event in events {
            assert_matches!(
                event,
                MockReactorEvent::FinalitySignatureFetcherRequest(FetcherRequest {
                    id,
                    ..
                }) => {
                    assert_eq!(id.block_hash(), block.hash());
                    assert_eq!(id.era_id(), block.header().era_id());
                }
            );
        }

        // Receive a late deploy response.
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            &mut rng,
            Event::DeployFetched {
                block_hash: *block.hash(),
                result: Either::Right(Ok(FetchedData::from_storage(Box::new(deploys[0].clone())))),
            },
        );

        assert_eq!(effects.len(), 0);
        latch_count_check(
            block_synchronizer.forward.as_ref(),
            expected_latch_count,
            "Latch should not have changed since we did not receive a new signature yet.",
        );
    }

    // Receive the rest of the the missing signatures to get strict finality. This would switch the
    // state to HaveStrictFinality and continue to request to make the block executable.
    {
        let mut generated_effects = Effects::new();
        for secret_key in validators_secret_keys.iter().rev().take(
            strict_finality_threshold(validators_secret_keys.len())
                - weak_finality_threshold(validators_secret_keys.len()),
        ) {
            // Register a finality signature
            let signature = FinalitySignature::create(
                *block.hash(),
                block.header().era_id(),
                secret_key.as_ref(),
            );
            assert!(signature.is_verified().is_ok());
            let effects = block_synchronizer.handle_event(
                mock_reactor.effect_builder(),
                &mut rng,
                Event::FinalitySignatureFetched(Ok(FetchedData::FromPeer {
                    item: Box::new(signature),
                    peer: peers[2],
                })),
            );
            generated_effects.extend(effects);
        }

        // Once strict finality is achieved, the synchronizer will try to make the block executable.
        let events = mock_reactor
            .process_effects(generated_effects.into_iter().rev().take(1))
            .await;

        for event in events {
            assert_matches!(
                event,
                MockReactorEvent::MakeBlockExecutableRequest(MakeBlockExecutableRequest {
                    block_hash,
                    ..
                }) if block_hash == *block.hash()
            );
        }

        latch_count_check(
            block_synchronizer.forward.as_ref(),
            1,
            "Latch count should still be 1 since no FinalizedBlock was received.",
        );
    }
}

#[tokio::test]
async fn historical_sync_latch_should_not_decrement_for_old_deploy_fetch_responses() {
    let rng = &mut TestRng::new();
    let mock_reactor = MockReactor::new();
    let first_deploy = Deploy::random(rng);
    let second_deploy = Deploy::random(rng);
    let third_deploy = Deploy::random(rng);
    let test_env = TestEnv::random(rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .deploys(
                [
                    first_deploy.clone(),
                    second_deploy.clone(),
                    third_deploy.clone(),
                ]
                .iter(),
            )
            .build(rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(rng, validator_matrix, Default::default())
            .with_legacy_finality(LegacyRequiredFinality::Strict);

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let historical_builder = block_synchronizer
        .historical
        .as_mut()
        .expect("Historical builder should have been initialized");
    historical_builder
        .register_block_header(block.clone().take_header(), None)
        .expect("header registration works");
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(historical_builder
        .register_block(block.into(), None)
        .is_ok());

    let _effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynced {
            block_hash: *block.hash(),
            result: Ok(GlobalStateSynchronizerResponse::new(
                super::global_state_synchronizer::RootHash::new(*block.state_root_hash()),
                vec![],
            )),
        },
    );

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    let _effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GotExecutionResultsChecksum {
            block_hash: *block.hash(),
            result: Ok(Some(Digest::SENTINEL_NONE)),
        },
    );

    let execution_results =
        BlockExecutionResultsOrChunk::new_mock_value_with_multiple_random_results(
            rng,
            *block.hash(),
            3,
        );
    let _effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsFetched {
            block_hash: *block.hash(),
            result: Ok(FetchedData::from_storage(Box::new(execution_results))),
        },
    );

    let _effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsStored(*block.hash()),
    );

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveAllExecutionResults(_, _, _, checksum)
            if checksum.is_checkable() == true
    );

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ApprovalsHashesFetched(Ok(FetchedData::from_storage(Box::new(
            ApprovalsHashes::new(
                block.hash(),
                vec![
                    first_deploy.compute_approvals_hash().unwrap(),
                    second_deploy.compute_approvals_hash().unwrap(),
                    third_deploy.compute_approvals_hash().unwrap(),
                ],
                dummy_merkle_proof(),
            ),
        )))),
    );

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveApprovalsHashes(_, _, _)
    );

    let events = mock_reactor.process_effects(effects).await;
    for event in events {
        assert_matches!(
            event,
            MockReactorEvent::DeployFetcherRequest(FetcherRequest { .. })
        );
    }

    latch_count_check(
        block_synchronizer.historical.as_ref(),
        MAX_SIMULTANEOUS_PEERS,
        format!(
            "Latch count should be {} since no deploys were received.",
            MAX_SIMULTANEOUS_PEERS
        )
        .as_str(),
    );

    // Receive 1 out of MAX_SIMULTANEOUS_PEERS requests for the first deploy.
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::DeployFetched {
            block_hash: *block.hash(),
            result: Either::Right(Ok(FetchedData::from_storage(Box::new(
                first_deploy.clone(),
            )))),
        },
    );

    // The first deploy was registered. The synchronizer will create MAX_SIMULTANEOUS_PEERS fetch
    // requests for another deploy.
    for event in mock_reactor.process_effects(effects).await {
        assert_matches!(
            event,
            MockReactorEvent::DeployFetcherRequest(FetcherRequest { .. })
        );
    }
    latch_count_check(
        block_synchronizer.historical.as_ref(),
        MAX_SIMULTANEOUS_PEERS,
        format!(
            "Latch count should be {} since the node should ask for the second deploy.",
            MAX_SIMULTANEOUS_PEERS
        )
        .as_str(),
    );

    // Receive 1 out of MAX_SIMULTANEOUS_PEERS requests for the second deploy.
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::DeployFetched {
            block_hash: *block.hash(),
            result: Either::Right(Ok(FetchedData::from_storage(Box::new(
                second_deploy.clone(),
            )))),
        },
    );

    // The second deploy was registered. The synchronizer will create MAX_SIMULTANEOUS_PEERS fetch
    // requests for another deploy.
    for event in mock_reactor.process_effects(effects).await {
        assert_matches!(
            event,
            MockReactorEvent::DeployFetcherRequest(FetcherRequest { .. })
        );
    }
    latch_count_check(
        block_synchronizer.historical.as_ref(),
        MAX_SIMULTANEOUS_PEERS,
        format!(
            "Latch count should be {} since the node should ask for the third deploy.",
            MAX_SIMULTANEOUS_PEERS
        )
        .as_str(),
    );

    // The current state is:
    // * Sent out MAX_SIMULTANEOUS_PEERS requests for the first deploy and received 1 response.
    // * Sent out MAX_SIMULTANEOUS_PEERS requests for the second deploy and received 1 response.
    // * Sent out MAX_SIMULTANEOUS_PEERS requests for the third deploy and haven't received anything
    //   yet.
    //
    // So we can receive at this point MAX_SIMULTANEOUS_PEERS - 2 "late" responses for the first and
    // second deploys and MAX_SIMULTANEOUS_PEERS responses for the third deploy.
    //
    // Simulate that we receive the "late" responses first. The synchronizer shouldn't unlatch and
    // try to send out more requests for the third deploy. It should hold off until the right
    // response comes through.

    // Receive the late responses for the first deploy
    for _ in 1..MAX_SIMULTANEOUS_PEERS {
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            rng,
            Event::DeployFetched {
                block_hash: *block.hash(),
                result: Either::Right(Ok(FetchedData::from_storage(Box::new(
                    first_deploy.clone(),
                )))),
            },
        );

        assert_eq!(effects.len(), 0);

        latch_count_check(
            block_synchronizer.historical.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            "Shouldn't decrement the latch since this was a late response",
        );
    }

    // Receive the late responses for the second deploy
    for _ in 1..MAX_SIMULTANEOUS_PEERS {
        let effects = block_synchronizer.handle_event(
            mock_reactor.effect_builder(),
            rng,
            Event::DeployFetched {
                block_hash: *block.hash(),
                result: Either::Right(Ok(FetchedData::from_storage(Box::new(
                    second_deploy.clone(),
                )))),
            },
        );

        assert_eq!(effects.len(), 0);

        latch_count_check(
            block_synchronizer.historical.as_ref(),
            MAX_SIMULTANEOUS_PEERS,
            "Shouldn't decrement the latch since this was a late response",
        );
    }

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::DeployFetched {
            block_hash: *block.hash(),
            result: Either::Right(Ok(FetchedData::from_storage(Box::new(
                third_deploy.clone(),
            )))),
        },
    );

    // ----- HaveAllDeploys -----
    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveAllDeploys(_, _)
    );

    let events = mock_reactor.process_effects(effects).await;
    for event in events {
        assert_matches!(event, MockReactorEvent::FinalitySignatureFetcherRequest(_));
    }
}

#[tokio::test]
async fn historical_sync_latch_should_not_decrement_for_old_execution_results() {
    let rng = &mut TestRng::new();
    let mock_reactor = MockReactor::new();
    let first_deploy = Deploy::random(rng);
    let second_deploy = Deploy::random(rng);
    let third_deploy = Deploy::random(rng);
    let test_env = TestEnv::random(rng).with_block(
        TestBlockBuilder::new()
            .era(1)
            .deploys(
                [
                    first_deploy.clone(),
                    second_deploy.clone(),
                    third_deploy.clone(),
                ]
                .iter(),
            )
            .build(rng),
    );
    let peers = test_env.peers();
    let block = test_env.block();
    let validator_matrix = test_env.gen_validator_matrix();
    let validators_secret_keys = test_env.validator_keys();
    let mut block_synchronizer =
        BlockSynchronizer::new_initialized(rng, validator_matrix, Default::default())
            .with_legacy_finality(LegacyRequiredFinality::Strict);

    // Register block for historical sync
    assert!(block_synchronizer.register_block_by_hash(*block.hash(), SHOULD_FETCH_EXECUTION_STATE));
    block_synchronizer.register_peers(*block.hash(), peers.clone());

    let historical_builder = block_synchronizer
        .historical
        .as_mut()
        .expect("Historical builder should have been initialized");
    historical_builder
        .register_block_header(block.clone().take_header(), None)
        .expect("header registration works");
    historical_builder.register_era_validator_weights(&block_synchronizer.validator_matrix);
    register_multiple_signatures(
        historical_builder,
        block,
        validators_secret_keys
            .iter()
            .take(weak_finality_threshold(validators_secret_keys.len())),
    );
    assert!(historical_builder
        .register_block(block.into(), None)
        .is_ok());

    let _effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GlobalStateSynced {
            block_hash: *block.hash(),
            result: Ok(GlobalStateSynchronizerResponse::new(
                super::global_state_synchronizer::RootHash::new(*block.state_root_hash()),
                vec![],
            )),
        },
    );

    assert_matches!(
        historical_state(&block_synchronizer),
        BlockAcquisitionState::HaveGlobalState { .. }
    );

    latch_count_check(
        block_synchronizer.historical.as_ref(),
        1,
        "Latch count should be 1 since we're waiting for execution results checksum.",
    );

    // Create chunked execution results.
    let execution_results =
        BlockExecutionResultsOrChunk::new_mock_value_with_multiple_random_results(
            rng,
            *block.hash(),
            100000, // Lots of results to achieve chunking.
        );
    let checksum = assert_matches!(
        execution_results.value(),
        ValueOrChunk::ChunkWithProof(chunk) => chunk.proof().root_hash()
    );

    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::GotExecutionResultsChecksum {
            block_hash: *block.hash(),
            result: Ok(Some(checksum)),
        },
    );

    for event in mock_reactor.process_effects(effects).await {
        assert_matches!(
            event,
            MockReactorEvent::BlockExecutionResultsOrChunkFetcherRequest(FetcherRequest { .. })
        );
    }

    latch_count_check(
        block_synchronizer.historical.as_ref(),
        MAX_SIMULTANEOUS_PEERS,
        format!(
            "Latch count should be {} since no chunks of execution results were received.",
            MAX_SIMULTANEOUS_PEERS
        )
        .as_str(),
    );

    // Receive the first chunk of execution results.
    let effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsFetched {
            block_hash: *block.hash(),
            result: Ok(FetchedData::from_storage(Box::new(
                execution_results.clone(),
            ))),
        },
    );

    // It's expected that the synchronizer will ask for the next chunks of execution results.
    for event in mock_reactor.process_effects(effects).await {
        assert_matches!(
            event,
            MockReactorEvent::BlockExecutionResultsOrChunkFetcherRequest(FetcherRequest { id, .. }) if id.chunk_index() != 0
        );
    }

    latch_count_check(
        block_synchronizer.historical.as_ref(),
        MAX_SIMULTANEOUS_PEERS,
        format!(
            "Latch count should be {} since no responses with chunks != 0 were received.",
            MAX_SIMULTANEOUS_PEERS
        )
        .as_str(),
    );

    // Receive the first chunk of execution results again (late response).
    let _effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsFetched {
            block_hash: *block.hash(),
            result: Ok(FetchedData::from_storage(Box::new(execution_results))),
        },
    );

    latch_count_check(
        block_synchronizer.historical.as_ref(),
        MAX_SIMULTANEOUS_PEERS,
        format!(
            "Latch count should be {} since we already had the first chunk and no responses with chunks != 0 were received.",
            MAX_SIMULTANEOUS_PEERS
        )
        .as_str(),
    );

    // Receive a fetch error.
    let _effects = block_synchronizer.handle_event(
        mock_reactor.effect_builder(),
        rng,
        Event::ExecutionResultsFetched {
            block_hash: *block.hash(),
            result: Err(FetcherError::Absent {
                id: Box::new(BlockExecutionResultsOrChunkId::new(*block.hash())),
                peer: peers[0],
            }),
        },
    );

    latch_count_check(
        block_synchronizer.historical.as_ref(),
        MAX_SIMULTANEOUS_PEERS - 1,
        format!(
            "Latch count should be {} since we received an `Absent` response.",
            MAX_SIMULTANEOUS_PEERS - 1
        )
        .as_str(),
    );
}
