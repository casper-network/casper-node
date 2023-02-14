use std::{
    collections::BTreeSet,
    fmt::{self, Debug, Display, Formatter},
    sync::Arc,
    time::Duration,
};

use derive_more::From;
use num_rational::Ratio;
use prometheus::Registry;
use rand::Rng;
use reactor::ReactorEvent;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error as ThisError;

use casper_types::{
    generate_ed25519_keypair, testing::TestRng, ProtocolVersion, PublicKey, SecretKey, Signature,
    U512,
};
use tokio::time;

use super::*;
use crate::{
    components::{
        consensus::tests::utils::{ALICE_NODE_ID, ALICE_PUBLIC_KEY, ALICE_SECRET_KEY, BOB_NODE_ID},
        network::Identity as NetworkIdentity,
        storage::{self, Storage},
    },
    effect::{
        announcements::ControlAnnouncement,
        requests::{BlockCompleteConfirmationRequest, ContractRuntimeRequest, NetworkRequest},
    },
    protocol::Message,
    reactor::{self, EventQueueHandle, QueueKind, Reactor, Runner, TryCrankOutcome},
    types::{Block, Chainspec, ChainspecRawBytes, EraValidatorWeights},
    utils::{Loadable, WithDir},
    NodeRng,
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const RECENT_ERA_INTERVAL: u64 = 1;

fn meta_block_with_default_state(block: Arc<Block>) -> MetaBlock {
    MetaBlock::new(block, vec![], MetaBlockState::new())
}

fn signatures_for_block(block: &Block, signatures: &Vec<FinalitySignature>) -> BlockSignatures {
    let mut block_signatures = BlockSignatures::new(*block.hash(), block.header().era_id());
    for signature in signatures {
        block_signatures.insert_proof(signature.public_key.clone(), signature.signature);
    }
    block_signatures
}

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[allow(clippy::large_enum_variant)]
#[must_use]
enum Event {
    #[from]
    Storage(#[serde(skip_serializing)] storage::Event),
    #[from]
    BlockAccumulator(#[serde(skip_serializing)] super::Event),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    FatalAnnouncement(FatalAnnouncement),
    #[from]
    BlockAccumulatorAnnouncement(#[serde(skip_serializing)] BlockAccumulatorAnnouncement),
    #[from]
    MetaBlockAnnouncement(#[serde(skip_serializing)] MetaBlockAnnouncement),
    #[from]
    ContractRuntime(#[serde(skip_serializing)] ContractRuntimeRequest),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    NetworkRequest(NetworkRequest<Message>),
    #[from]
    NetworkPeerBehaviorAnnouncement(PeerBehaviorAnnouncement),
}

impl From<BlockCompleteConfirmationRequest> for Event {
    fn from(request: BlockCompleteConfirmationRequest) -> Self {
        Event::Storage(storage::Event::MarkBlockCompletedRequest(request))
    }
}

impl ReactorEvent for Event {
    fn is_control(&self) -> bool {
        matches!(self, Event::ControlAnnouncement(_))
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::BlockAccumulator(event) => write!(formatter, "block accumulator: {}", event),
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::FatalAnnouncement(fatal_ann) => write!(formatter, "fatal: {}", fatal_ann),
            Event::BlockAccumulatorAnnouncement(ann) => {
                write!(formatter, "block-accumulator announcement: {}", ann)
            }
            Event::MetaBlockAnnouncement(meta_block_ann) => {
                write!(formatter, "meta block announcement: {}", meta_block_ann)
            }
            Event::ContractRuntime(event) => {
                write!(formatter, "contract-runtime event: {:?}", event)
            }
            Event::StorageRequest(request) => write!(formatter, "storage request: {:?}", request),
            Event::NetworkRequest(request) => write!(formatter, "network request: {:?}", request),
            Event::NetworkPeerBehaviorAnnouncement(peer_behavior) => {
                write!(formatter, "peer behavior announcement: {:?}", peer_behavior)
            }
        }
    }
}

/// Error type returned by the test reactor.
#[derive(Debug, ThisError)]
enum ReactorError {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

struct MockReactor {
    storage: Storage,
    block_accumulator: BlockAccumulator,
    blocked_peers: Vec<PeerBehaviorAnnouncement>,
    validator_matrix: ValidatorMatrix,
    _storage_tempdir: TempDir,
}

impl Reactor for MockReactor {
    type Event = Event;
    type Config = ();
    type Error = ReactorError;

    fn new(
        _config: Self::Config,
        chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        _event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (storage_config, storage_tempdir) = storage::Config::default_for_tests();
        let storage_withdir = WithDir::new(storage_tempdir.path(), storage_config);
        let validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
        let block_accumulator_config = Config::default();
        let block_time = block_accumulator_config.purge_interval() / 2;

        let block_accumulator = BlockAccumulator::new(
            block_accumulator_config,
            validator_matrix.clone(),
            RECENT_ERA_INTERVAL,
            block_time,
            registry,
        )
        .unwrap();

        let storage = Storage::new(
            &storage_withdir,
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            "test",
            chainspec.deploy_config.max_ttl,
            chainspec.core_config.recent_era_count(),
            Some(registry),
            false,
        )
        .unwrap();

        let reactor = MockReactor {
            storage,
            block_accumulator,
            blocked_peers: vec![],
            validator_matrix,
            _storage_tempdir: storage_tempdir,
        };

        let effects = Effects::new();

        Ok((reactor, effects))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::StorageRequest(req) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            Event::BlockAccumulator(event) => reactor::wrap_effects(
                Event::BlockAccumulator,
                self.block_accumulator
                    .handle_event(effect_builder, rng, event),
            ),
            Event::MetaBlockAnnouncement(MetaBlockAnnouncement(mut meta_block)) => {
                let effects = Effects::new();
                let state = &mut meta_block.state;
                assert!(state.is_stored());
                state.register_as_sent_to_deploy_buffer();
                if !state.is_executed() {
                    return effects;
                }

                state.register_we_have_tried_to_sign();
                state.register_as_consensus_notified();

                if state.register_as_accumulator_notified().was_updated() {
                    return reactor::wrap_effects(
                        Event::BlockAccumulator,
                        self.block_accumulator.handle_event(
                            effect_builder,
                            rng,
                            super::Event::ExecutedBlock { meta_block },
                        ),
                    );
                }

                assert!(state.is_marked_complete());
                state.register_as_gossiped();
                assert!(state.verify_complete());
                effects
            }
            Event::ControlAnnouncement(ctrl_ann) => {
                panic!("unhandled control announcement: {}", ctrl_ann)
            }
            Event::FatalAnnouncement(fatal_ann) => {
                panic!("unhandled fatal announcement: {}", fatal_ann)
            }
            Event::BlockAccumulatorAnnouncement(_) => {
                // We do not care about block accumulator announcements in these tests.
                Effects::new()
            }
            Event::ContractRuntime(_event) => {
                panic!("test does not handle contract runtime events")
            }
            Event::NetworkRequest(_) => panic!("test does not handle network requests"),
            Event::NetworkPeerBehaviorAnnouncement(peer_behavior) => {
                self.blocked_peers.push(peer_behavior);
                Effects::new()
            }
        }
    }
}

#[test]
fn upsert_acceptor() {
    let mut rng = TestRng::new();
    let config = Config::default();
    let era0 = EraId::from(0);
    let validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    let recent_era_interval = 1;
    let block_time = config.purge_interval() / 2;
    let metrics_registry = Registry::new();
    let mut accumulator = BlockAccumulator::new(
        config,
        validator_matrix,
        recent_era_interval,
        block_time,
        &metrics_registry,
    )
    .unwrap();

    accumulator.register_local_tip(0, EraId::new(0));

    let max_block_count =
        PEER_RATE_LIMIT_MULTIPLIER * ((config.purge_interval() / block_time) as usize);

    for _ in 0..max_block_count {
        accumulator.upsert_acceptor(
            BlockHash::random(&mut rng),
            Some(era0),
            Some(*ALICE_NODE_ID),
        );
    }

    assert_eq!(accumulator.block_acceptors.len(), max_block_count);

    let block_hash = BlockHash::random(&mut rng);

    // Alice has sent us too many blocks; we don't register this one.
    accumulator.upsert_acceptor(block_hash, Some(era0), Some(*ALICE_NODE_ID));
    assert_eq!(accumulator.block_acceptors.len(), max_block_count);
    assert!(!accumulator.block_acceptors.contains_key(&block_hash));

    // Bob hasn't sent us anything yet. But we don't insert without an era ID.
    accumulator.upsert_acceptor(block_hash, None, Some(*BOB_NODE_ID));
    assert_eq!(accumulator.block_acceptors.len(), max_block_count);
    assert!(!accumulator.block_acceptors.contains_key(&block_hash));

    // With an era ID he's allowed to tell us about this one.
    accumulator.upsert_acceptor(block_hash, Some(era0), Some(*BOB_NODE_ID));
    assert_eq!(accumulator.block_acceptors.len(), max_block_count + 1);
    assert!(accumulator.block_acceptors.contains_key(&block_hash));

    // And if Alice tells us about it _now_, we'll register her as a peer.
    accumulator.upsert_acceptor(block_hash, None, Some(*ALICE_NODE_ID));
    assert!(accumulator.block_acceptors[&block_hash]
        .peers()
        .contains(&ALICE_NODE_ID));

    // Modify the timestamp of the acceptor we just added to be too old.
    let purge_interval = config.purge_interval() * 2;
    let purged_hash = {
        let (hash, timestamp) = accumulator
            .peer_block_timestamps
            .get_mut(&ALICE_NODE_ID)
            .unwrap()
            .front_mut()
            .unwrap();
        *timestamp = Timestamp::now().saturating_sub(purge_interval);
        *hash
    };
    // This should lead to a purge of said acceptor, therefore enabling us to
    // add another one for Alice.
    assert_eq!(accumulator.block_acceptors.len(), max_block_count + 1);
    accumulator.upsert_acceptor(
        BlockHash::random(&mut rng),
        Some(era0),
        Some(*ALICE_NODE_ID),
    );
    // Acceptor was added.
    assert_eq!(accumulator.block_acceptors.len(), max_block_count + 2);
    // The timestamp was purged.
    assert_ne!(
        accumulator
            .peer_block_timestamps
            .get(&ALICE_NODE_ID)
            .unwrap()
            .front()
            .unwrap()
            .0,
        purged_hash
    );
}

#[test]
fn acceptor_get_peers() {
    let mut rng = TestRng::new();
    let block = Block::random(&mut rng);
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);
    assert!(acceptor.peers().is_empty());
    let first_peer = NodeId::random(&mut rng);
    let second_peer = NodeId::random(&mut rng);
    acceptor.register_peer(first_peer);
    assert_eq!(acceptor.peers(), &BTreeSet::from([first_peer]));
    acceptor.register_peer(second_peer);
    assert_eq!(acceptor.peers(), &BTreeSet::from([first_peer, second_peer]));
}

#[test]
fn acceptor_register_finality_signature() {
    let mut rng = TestRng::new();
    // Create a block and an acceptor for it.
    let block = Arc::new(Block::random(&mut rng));
    let mut meta_block = MetaBlock::new(block.clone(), vec![], MetaBlockState::new());
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);

    // Create a finality signature with the wrong block hash.
    let wrong_fin_sig = FinalitySignature::random_for_block(BlockHash::random(&mut rng), 0);
    assert!(matches!(
        acceptor
            .register_finality_signature(wrong_fin_sig, None)
            .unwrap_err(),
        Error::BlockHashMismatch {
            expected: _,
            actual: _
        }
    ));

    // Create an invalid finality signature.
    let invalid_fin_sig = FinalitySignature::new(
        *block.hash(),
        rng.gen(),
        Signature::System,
        PublicKey::random(&mut rng),
    );
    // We shouldn't be able to create invalid signatures ourselves, so we've
    // reached an invalid state.
    assert!(matches!(
        acceptor
            .register_finality_signature(invalid_fin_sig.clone(), None)
            .unwrap_err(),
        Error::InvalidConfiguration
    ));
    // Peers shouldn't send us invalid signatures.
    let first_peer = NodeId::random(&mut rng);
    assert!(matches!(
        acceptor
            .register_finality_signature(invalid_fin_sig, Some(first_peer))
            .unwrap_err(),
        Error::InvalidGossip(_)
    ));
    // Create a valid finality signature and register it.
    let fin_sig =
        FinalitySignature::random_for_block(*block.hash(), block.header().era_id().into());
    assert!(acceptor
        .register_finality_signature(fin_sig.clone(), Some(first_peer))
        .unwrap()
        .is_none());
    // Register it from the second peer as well.
    let second_peer = NodeId::random(&mut rng);
    assert!(acceptor
        .register_finality_signature(fin_sig.clone(), Some(second_peer))
        .unwrap()
        .is_none());
    // Make sure the peer list is updated accordingly.
    let (sig, senders) = acceptor.signatures().get(&fin_sig.public_key).unwrap();
    assert_eq!(*sig, fin_sig);
    assert_eq!(*senders, BTreeSet::from([first_peer, second_peer]));
    // Create a second finality signature and register it.
    let second_fin_sig =
        FinalitySignature::random_for_block(*block.hash(), block.header().era_id().into());
    assert!(acceptor
        .register_finality_signature(second_fin_sig.clone(), Some(first_peer))
        .unwrap()
        .is_none());
    // Make sure the peer list for the first signature is unchanged.
    let (first_sig, first_sig_senders) = acceptor.signatures().get(&fin_sig.public_key).unwrap();
    assert_eq!(*first_sig, fin_sig);
    assert_eq!(
        *first_sig_senders,
        BTreeSet::from([first_peer, second_peer])
    );
    // Make sure the peer list for the second signature is correct.
    let (sig, senders) = acceptor
        .signatures()
        .get(&second_fin_sig.public_key)
        .unwrap();
    assert_eq!(*sig, second_fin_sig);
    assert_eq!(*senders, BTreeSet::from([first_peer]));
    assert!(!acceptor.has_sufficient_finality());
    // Register the block with the sufficient finality flag set.
    meta_block.state.register_has_sufficient_finality();
    acceptor
        .register_block(meta_block.clone(), Some(first_peer))
        .unwrap();
    // Registering invalid signatures should still yield an error.
    let mut invalid_fin_sig =
        FinalitySignature::random_for_block(*block.hash(), block.header().era_id().into());
    invalid_fin_sig.era_id = (u64::MAX ^ u64::from(invalid_fin_sig.era_id)).into();
    assert!(matches!(
        acceptor
            .register_finality_signature(invalid_fin_sig.clone(), Some(first_peer))
            .unwrap_err(),
        Error::EraMismatch {
            block_hash: _,
            expected: _,
            actual: _,
            peer: _
        }
    ));
    // Registering an invalid signature that we created means we're in an
    // invalid state.
    assert!(matches!(
        acceptor
            .register_finality_signature(invalid_fin_sig, None)
            .unwrap_err(),
        Error::InvalidConfiguration
    ));
    // Registering valid signatures still works, but we already had the second
    // signature.
    assert!(acceptor
        .register_finality_signature(second_fin_sig.clone(), Some(second_peer))
        .unwrap()
        .is_none());
    assert!(acceptor
        .signatures()
        .get(&second_fin_sig.public_key)
        .unwrap()
        .1
        .contains(&second_peer));
    // Register a new valid signature which should be yielded by the function.
    let third_fin_sig =
        FinalitySignature::random_for_block(*block.hash(), block.header().era_id().into());
    assert_eq!(
        acceptor
            .register_finality_signature(third_fin_sig.clone(), Some(first_peer))
            .unwrap()
            .unwrap(),
        third_fin_sig
    );
    // Additional registrations of the third signature with and without a peer
    // should still work.
    assert!(acceptor
        .register_finality_signature(third_fin_sig.clone(), Some(second_peer))
        .unwrap()
        .is_none());
    assert!(acceptor
        .register_finality_signature(third_fin_sig, None)
        .unwrap()
        .is_none());
}

#[test]
fn acceptor_register_block() {
    let mut rng = TestRng::new();
    // Create a block and an acceptor for it.
    let block = Arc::new(Block::random(&mut rng));
    let mut meta_block = meta_block_with_default_state(block.clone());
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);

    // Create a finality signature with the wrong block hash.
    let wrong_block = meta_block_with_default_state(Arc::new(Block::random(&mut rng)));
    assert!(matches!(
        acceptor.register_block(wrong_block, None).unwrap_err(),
        Error::BlockHashMismatch {
            expected: _,
            actual: _
        }
    ));

    {
        // Invalid block case.
        let invalid_block = Arc::new(Block::random_invalid(&mut rng));
        let mut invalid_block_acceptor = BlockAcceptor::new(*invalid_block.hash(), vec![]);
        let invalid_meta_block = meta_block_with_default_state(invalid_block);
        let malicious_peer = NodeId::random(&mut rng);
        // Peers shouldn't send us invalid blocks.
        assert!(matches!(
            invalid_block_acceptor
                .register_block(invalid_meta_block.clone(), Some(malicious_peer))
                .unwrap_err(),
            Error::InvalidGossip(_)
        ));
        // We shouldn't be able to create invalid blocks ourselves, so we've
        // reached an invalid state.
        assert!(matches!(
            invalid_block_acceptor
                .register_block(invalid_meta_block, None)
                .unwrap_err(),
            Error::InvalidConfiguration
        ));
    }

    // At this point, we know only the hash of the block.
    assert!(acceptor.block_height().is_none());
    assert!(acceptor.peers().is_empty());

    // Register the block with ourselves as source.
    acceptor.register_block(meta_block.clone(), None).unwrap();
    assert_eq!(acceptor.block_height().unwrap(), block.height());
    assert!(acceptor.peers().is_empty());

    // Register the block from a peer.
    let first_peer = NodeId::random(&mut rng);
    acceptor
        .register_block(meta_block.clone(), Some(first_peer))
        .unwrap();
    // Peer list should be updated.
    assert_eq!(*acceptor.peers(), BTreeSet::from([first_peer]));

    // The `executed` flag should not be set yet.
    assert!(!acceptor.executed());
    // Register the block from a second peer with the executed flag set.
    let second_peer = NodeId::random(&mut rng);
    assert!(meta_block.state.register_as_executed().was_updated());
    acceptor
        .register_block(meta_block.clone(), Some(second_peer))
        .unwrap();
    // Peer list should contain both peers.
    assert_eq!(*acceptor.peers(), BTreeSet::from([first_peer, second_peer]));
    // `executed` flag should now be set.
    assert!(acceptor.executed());

    // Re-registering with the `executed` flag set should not change anything.
    acceptor.register_block(meta_block, None).unwrap();
    assert_eq!(*acceptor.peers(), BTreeSet::from([first_peer, second_peer]));
    assert!(acceptor.executed());
}

#[test]
fn acceptor_should_store_block() {
    let mut rng = TestRng::new();
    // Create a block and an acceptor for it.
    let block = Arc::new(Block::random(&mut rng));
    let mut meta_block = meta_block_with_default_state(block.clone());
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);

    // Create 4 pairs of keys so we can later create 4 signatures.
    let keys: Vec<(SecretKey, PublicKey)> = (0..4)
        .into_iter()
        .map(|_| generate_ed25519_keypair())
        .collect();
    // Register the keys into the era validator weights, front loaded on the
    // first 2 with 80% weight.
    let era_validator_weights = EraValidatorWeights::new(
        block.header().era_id(),
        BTreeMap::from([
            (keys[0].1.clone(), U512::from(40)),
            (keys[1].1.clone(), U512::from(40)),
            (keys[2].1.clone(), U512::from(10)),
            (keys[3].1.clone(), U512::from(10)),
        ]),
        Ratio::new(1, 3),
    );

    // We should have nothing at this point.
    assert!(
        !acceptor.has_sufficient_finality()
            && acceptor.block_height().is_none()
            && acceptor.signatures().is_empty()
    );

    // With the sufficient finality flag set, nothing else should matter and we
    // should not store anything.
    acceptor.set_sufficient_finality(true);
    let (should_store, _offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);
    // Reset the flag.
    acceptor.set_sufficient_finality(false);

    let (should_store, offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);
    assert!(offenders.is_empty());

    let mut signatures = vec![];

    // Create the first validator's signature.
    let fin_sig = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &keys[0].0,
        keys[0].1.clone(),
    );
    signatures.push(fin_sig.clone());
    // First signature with 40% weight brings the block to weak finality.
    acceptor.register_finality_signature(fin_sig, None).unwrap();
    let (should_store, _offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Registering the block now.
    acceptor.register_block(meta_block.clone(), None).unwrap();
    let (should_store, _offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Create the third validator's signature.
    let fin_sig = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &keys[2].0,
        keys[2].1.clone(),
    );
    // The third signature with weight 10% doesn't make the block go to
    // strict finality.
    signatures.push(fin_sig.clone());
    acceptor.register_finality_signature(fin_sig, None).unwrap();
    let (should_store, _offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Create a bogus signature from a non-validator for this era.
    let non_validator_keys = generate_ed25519_keypair();
    let faulty_peer = NodeId::random(&mut rng);
    let bogus_sig = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &non_validator_keys.0,
        non_validator_keys.1.clone(),
    );
    acceptor
        .register_finality_signature(bogus_sig, Some(faulty_peer))
        .unwrap();
    let (should_store, offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);
    // Make sure the peer who sent us this bogus signature is marked as an
    // offender.
    assert_eq!(offenders[0].0, faulty_peer);

    // Create the second validator's signature.
    let fin_sig = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &keys[1].0,
        keys[1].1.clone(),
    );
    signatures.push(fin_sig.clone());
    // Second signature with 40% weight brings the block to strict finality.
    acceptor.register_finality_signature(fin_sig, None).unwrap();
    let (should_store, _offenders) = acceptor.should_store_block(&era_validator_weights);
    let block_signatures = signatures_for_block(&block, &signatures);
    let mut meta_block_with_expected_state = meta_block.clone();
    meta_block_with_expected_state.state.register_as_stored();
    meta_block_with_expected_state
        .state
        .register_has_sufficient_finality();
    assert_eq!(
        should_store,
        ShouldStore::SufficientlySignedBlock {
            meta_block: meta_block_with_expected_state,
            block_signatures,
        }
    );

    // Create the fourth validator's signature.
    let fin_sig = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &keys[3].0,
        keys[3].1.clone(),
    );
    // Already have sufficient finality signatures, so we're not supposed to
    // store anything else.
    acceptor.register_finality_signature(fin_sig, None).unwrap();
    let (should_store, _offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Without the block, even with sufficient signatures we should not store anything.
    acceptor.set_meta_block(None);
    acceptor.set_sufficient_finality(false);
    let (should_store, _offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Without any signatures, we should not store anything.
    meta_block.state.register_has_sufficient_finality();
    acceptor.set_meta_block(Some(meta_block));
    acceptor.signatures_mut().retain(|_, _| false);
    let (should_store, _offenders) = acceptor.should_store_block(&era_validator_weights);
    assert_eq!(should_store, ShouldStore::Nothing);
}

#[test]
fn accumulator_highest_usable_block_height() {
    let mut rng = TestRng::new();
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    let block_accumulator_config = Config::default();
    let recent_era_interval = 1;
    let block_time = block_accumulator_config.purge_interval() / 2;
    let mut block_accumulator = BlockAccumulator::new(
        block_accumulator_config,
        validator_matrix.clone(),
        recent_era_interval,
        block_time,
        &Registry::default(),
    )
    .unwrap();

    // Create 3 parent-child blocks.
    let block_1 = Arc::new(generate_non_genesis_block(&mut rng));
    let block_2 = Arc::new(generate_next_block(&mut rng, &block_1));
    let block_3 = Arc::new(generate_next_block(&mut rng, &block_2));

    // One finality signature from our only validator for block 1.
    let fin_sig_1 = FinalitySignature::create(
        *block_1.hash(),
        block_1.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    // One finality signature from our only validator for block 2.
    let fin_sig_2 = FinalitySignature::create(
        *block_2.hash(),
        block_2.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    // One finality signature from our only validator for block 3.
    let fin_sig_3 = FinalitySignature::create(
        *block_3.hash(),
        block_3.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );

    // Register the eras in the validator matrix so the blocks are valid.
    {
        register_evw_for_era(&mut validator_matrix, block_1.header().era_id());
        register_evw_for_era(&mut validator_matrix, block_2.header().era_id());
        register_evw_for_era(&mut validator_matrix, block_3.header().era_id());
    }

    // The accumulator should have no usable block height at inception.
    assert!(block_accumulator.highest_usable_block_height().is_none());

    // Create an empty acceptor and insert it into the accumulator.
    let acceptor = BlockAcceptor::new(*block_2.hash(), vec![]);
    block_accumulator
        .block_acceptors
        .insert(*block_2.hash(), acceptor);
    // An empty acceptor should not count towards the usable block height.
    assert!(block_accumulator.highest_usable_block_height().is_none());

    {
        // Insert the second block with sufficient finality.
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(block_2.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new(block_2.clone(), vec![], state);
        acceptor
            .register_finality_signature(fin_sig_2, None)
            .unwrap();
        acceptor.register_block(meta_block, None).unwrap();
    }
    // Now we should have a usable block height.
    assert_eq!(
        block_accumulator.highest_usable_block_height().unwrap(),
        block_2.height()
    );

    {
        // Insert the first block with sufficient finality.
        let mut acceptor = BlockAcceptor::new(*block_1.hash(), vec![]);
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new(block_1.clone(), vec![], state);
        acceptor
            .register_finality_signature(fin_sig_1, None)
            .unwrap();
        acceptor.register_block(meta_block, None).unwrap();
        block_accumulator
            .block_acceptors
            .insert(*block_1.hash(), acceptor);
    }
    // The first block has a lower height than the second, so the highest
    // usable block height should still be the second block's height.
    assert_eq!(
        block_accumulator.highest_usable_block_height().unwrap(),
        block_2.height()
    );

    {
        // Insert the third block with sufficient finality.
        let mut acceptor = BlockAcceptor::new(*block_3.hash(), vec![]);
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new(block_3.clone(), vec![], state);
        acceptor
            .register_finality_signature(fin_sig_3, None)
            .unwrap();
        acceptor.register_block(meta_block, None).unwrap();
        block_accumulator
            .block_acceptors
            .insert(*block_3.hash(), acceptor);
    }
    // The third block has a higher height than the second, so the highest
    // usable block height should now be the third block's height.
    assert_eq!(
        block_accumulator.highest_usable_block_height().unwrap(),
        block_3.height()
    );
}

#[test]
fn accumulator_should_leap() {
    let mut rng = TestRng::new();
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    let block_accumulator_config = Config::default();
    let recent_era_interval = 1;
    let block_time = block_accumulator_config.purge_interval() / 2;
    let attempt_execution_threshold = block_accumulator_config.attempt_execution_threshold();
    let mut block_accumulator = BlockAccumulator::new(
        block_accumulator_config,
        validator_matrix.clone(),
        recent_era_interval,
        block_time,
        &Registry::default(),
    )
    .unwrap();

    // Create a block.
    let starting_seed: u64 = rng.gen_range(1..10);
    let block = Arc::new(Block::random_with_specifics(
        &mut rng,
        EraId::from(starting_seed),
        attempt_execution_threshold + starting_seed,
        ProtocolVersion::V1_0_0,
        starting_seed % 2 == 0,
        None,
    ));

    // One finality signature from our only validator for block 1.
    let fin_sig = FinalitySignature::create(
        *block.hash(),
        block.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );

    // Register the era in the validator matrix so the block is valid.
    register_evw_for_era(&mut validator_matrix, block.header().era_id());

    // The accumulator should try to leap at inception, no matter the starting
    // height.
    assert!(block_accumulator.should_leap(0));
    assert!(block_accumulator.should_leap(block.height().saturating_add(1_000)));

    // Create an acceptor to change the highest usable block height.
    {
        // Insert the block with sufficient finality.
        let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new(block.clone(), vec![], state);
        acceptor.register_finality_signature(fin_sig, None).unwrap();
        acceptor.register_block(meta_block, None).unwrap();
        block_accumulator
            .block_acceptors
            .insert(*block.hash(), acceptor);
    }
    // Highest usable block height should have changed.
    let highest_usable_block_height = block_accumulator.highest_usable_block_height().unwrap();
    assert_eq!(highest_usable_block_height, block.height());
    // We should not leap from a height that is greater than our highest usable
    // block height.
    assert!(!block_accumulator.should_leap(highest_usable_block_height + 1));

    // We should leap from a height *lower* than `attempt_execution_threshold`
    // from the highest usable block height.
    assert!(
        !block_accumulator.should_leap(highest_usable_block_height - attempt_execution_threshold)
    );
    assert!(!block_accumulator
        .should_leap(highest_usable_block_height - attempt_execution_threshold + 1));
    assert!(block_accumulator
        .should_leap(highest_usable_block_height - attempt_execution_threshold - 1));
}

#[test]
fn accumulator_purge() {
    let mut rng = TestRng::new();
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    let block_accumulator_config = Config::default();
    let recent_era_interval = 1;
    let block_time = block_accumulator_config.purge_interval() / 2;
    let purge_interval = block_accumulator_config.purge_interval();
    let time_before_insertion = Timestamp::now();
    let mut block_accumulator = BlockAccumulator::new(
        block_accumulator_config,
        validator_matrix.clone(),
        recent_era_interval,
        block_time,
        &Registry::default(),
    )
    .unwrap();
    block_accumulator.register_local_tip(0, 0.into());

    // Create 3 parent-child blocks.
    let block_1 = Arc::new(generate_non_genesis_block(&mut rng));
    let block_2 = Arc::new(generate_next_block(&mut rng, &block_1));
    let block_3 = Arc::new(generate_next_block(&mut rng, &block_2));

    // Also create 2 peers.
    let peer_1 = NodeId::random(&mut rng);
    let peer_2 = NodeId::random(&mut rng);

    // One finality signature from our only validator for block 1.
    let fin_sig_1 = FinalitySignature::create(
        *block_1.hash(),
        block_1.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    // One finality signature from our only validator for block 2.
    let fin_sig_2 = FinalitySignature::create(
        *block_2.hash(),
        block_2.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    // One finality signature from our only validator for block 3.
    let fin_sig_3 = FinalitySignature::create(
        *block_3.hash(),
        block_3.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );

    // Register the eras in the validator matrix so the blocks are valid.
    {
        register_evw_for_era(&mut validator_matrix, block_1.header().era_id());
        register_evw_for_era(&mut validator_matrix, block_2.header().era_id());
        register_evw_for_era(&mut validator_matrix, block_3.header().era_id());
    }

    // We will manually call `upsert_acceptor` in order to have
    // `peer_block_timestamps` populated.
    {
        // Insert the first block with sufficient finality from the first peer.
        block_accumulator.upsert_acceptor(
            *block_1.hash(),
            Some(block_1.header().era_id()),
            Some(peer_1),
        );
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(block_1.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new(block_1.clone(), vec![], state);
        acceptor
            .register_finality_signature(fin_sig_1, Some(peer_1))
            .unwrap();
        acceptor.register_block(meta_block, None).unwrap();
    }

    {
        // Insert the second block with sufficient finality from the second
        // peer.
        block_accumulator.upsert_acceptor(
            *block_2.hash(),
            Some(block_2.header().era_id()),
            Some(peer_2),
        );
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(block_2.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new(block_2.clone(), vec![], state);
        acceptor
            .register_finality_signature(fin_sig_2, Some(peer_2))
            .unwrap();
        acceptor.register_block(meta_block, None).unwrap();
    }

    {
        // Insert the third block with sufficient finality from the third peer.
        block_accumulator.upsert_acceptor(
            *block_3.hash(),
            Some(block_3.header().era_id()),
            Some(peer_1),
        );
        block_accumulator.upsert_acceptor(
            *block_3.hash(),
            Some(block_3.header().era_id()),
            Some(peer_2),
        );
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(block_3.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new(block_3.clone(), vec![], state);
        acceptor
            .register_finality_signature(fin_sig_3, Some(peer_1))
            .unwrap();
        acceptor.register_block(meta_block, Some(peer_2)).unwrap();
    }

    {
        // Modify the times in the acceptors for blocks 1 and 2 as well as in
        // `peer_block_timestamps` for the second peer to become outdated.
        let last_progress = time_before_insertion.saturating_sub(purge_interval * 10);
        block_accumulator
            .block_acceptors
            .get_mut(block_1.hash())
            .unwrap()
            .set_last_progress(last_progress);
        block_accumulator
            .block_acceptors
            .get_mut(block_2.hash())
            .unwrap()
            .set_last_progress(last_progress);
        for (_block_hash, timestamp) in block_accumulator
            .peer_block_timestamps
            .get_mut(&peer_2)
            .unwrap()
        {
            *timestamp = last_progress;
        }
    }

    // Entries we modified earlier should be purged.
    block_accumulator.purge();
    // Acceptors for blocks 1 and 2 should have been purged.
    assert!(!block_accumulator
        .block_acceptors
        .contains_key(block_1.hash()));
    assert!(!block_accumulator
        .block_acceptors
        .contains_key(block_2.hash()));
    assert!(block_accumulator
        .block_acceptors
        .contains_key(block_3.hash()));
    // The third block acceptor is all that is left and it has no known
    // children, so `block_children` should be empty.
    assert!(block_accumulator.block_children.is_empty());
    // We should have kept only the timestamps for the first peer.
    assert!(block_accumulator
        .peer_block_timestamps
        .contains_key(&peer_1));
    assert!(!block_accumulator
        .peer_block_timestamps
        .contains_key(&peer_2));
}

fn register_evw_for_era(validator_matrix: &mut ValidatorMatrix, era_id: EraId) {
    let weights = EraValidatorWeights::new(
        era_id,
        BTreeMap::from([(ALICE_PUBLIC_KEY.clone(), 100.into())]),
        Ratio::new(1, 3),
    );
    validator_matrix.register_era_validator_weights(weights);
}

fn generate_next_block(rng: &mut TestRng, block: &Block) -> Block {
    let era_id = if block.header().is_switch_block() {
        block.header().era_id().successor()
    } else {
        block.header().era_id()
    };
    Block::random_with_specifics(
        rng,
        era_id,
        // Safe because generated heights can't get to `u64::MAX`.
        block.header().height() + 1,
        block.protocol_version(),
        false,
        None,
    )
}

fn generate_non_genesis_block(rng: &mut TestRng) -> Block {
    let era = rng.gen_range(10..20);
    let height = era * 10 + rng.gen_range(0..10);
    let is_switch = rng.gen_bool(0.1);

    Block::random_with_specifics(
        rng,
        EraId::from(era),
        height,
        ProtocolVersion::V1_0_0,
        is_switch,
        None,
    )
}

fn generate_older_block(rng: &mut TestRng, block: &Block, height_difference: u64) -> Block {
    Block::random_with_specifics(
        rng,
        block.header().era_id().predecessor().unwrap_or_default(),
        block.header().height() - height_difference,
        block.protocol_version(),
        false,
        None,
    )
}

#[tokio::test]
async fn block_accumulator_reactor_flow() {
    let mut rng = TestRng::new();
    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let mut runner: Runner<MockReactor> = Runner::new(
        (),
        Arc::new(chainspec),
        Arc::new(chainspec_raw_bytes),
        &mut rng,
    )
    .await
    .unwrap();

    // Create 2 blocks, one parent one child.
    let block_1 = generate_non_genesis_block(&mut rng);
    let block_2 = generate_next_block(&mut rng, &block_1);

    // Also create 2 peers.
    let peer_1 = NodeId::random(&mut rng);
    let peer_2 = NodeId::random(&mut rng);

    // One finality signature from our only validator for block 1.
    let fin_sig_1 = FinalitySignature::create(
        *block_1.hash(),
        block_1.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    // One finality signature from our only validator for block 2.
    let fin_sig_2 = FinalitySignature::create(
        *block_2.hash(),
        block_2.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );

    // Register the eras in the validator matrix so the blocks are valid.
    {
        let mut validator_matrix = runner.reactor_mut().validator_matrix.clone();
        register_evw_for_era(&mut validator_matrix, block_1.header().era_id());
        register_evw_for_era(&mut validator_matrix, block_2.header().era_id());
    }

    // Register a signature for block 1.
    {
        let effect_builder = runner.effect_builder();
        let reactor = runner.reactor_mut();

        let block_accumulator = &mut reactor.block_accumulator;
        block_accumulator.register_local_tip(0, 0.into());

        let event = super::Event::ReceivedFinalitySignature {
            finality_signature: Box::new(fin_sig_1.clone()),
            sender: peer_1,
        };
        let effects = block_accumulator.handle_event(effect_builder, &mut rng, event);
        assert!(effects.is_empty());
    }

    // Register block 1.
    {
        runner
            .process_injected_effects(|effect_builder| {
                let event = super::Event::ReceivedBlock {
                    block: Arc::new(block_1.clone()),
                    sender: peer_2,
                };
                effect_builder
                    .into_inner()
                    .schedule(event, QueueKind::Validation)
                    .ignore()
            })
            .await;
        for _ in 0..6 {
            while runner.try_crank(&mut rng).await == TryCrankOutcome::NoEventsToProcess {
                time::sleep(POLL_INTERVAL).await;
            }
        }
        let expected_block = runner
            .reactor()
            .storage
            .read_block(block_1.hash())
            .unwrap()
            .unwrap();
        assert_eq!(expected_block, block_1);
        let expected_block_signatures = runner
            .reactor()
            .storage
            .get_finality_signatures_for_block(*block_1.hash());
        assert_eq!(
            expected_block_signatures
                .and_then(|sigs| sigs.get_finality_signature(&fin_sig_1.public_key))
                .unwrap(),
            fin_sig_1
        );
    }

    // Register block 2 before the signature.
    {
        let effect_builder = runner.effect_builder();
        let reactor = runner.reactor_mut();

        let block_accumulator = &mut reactor.block_accumulator;
        let event = super::Event::ReceivedBlock {
            block: Arc::new(block_2.clone()),
            sender: peer_2,
        };
        let effects = block_accumulator.handle_event(effect_builder, &mut rng, event);
        assert!(effects.is_empty());
    }

    // Register the signature for block 2.
    {
        runner
            .process_injected_effects(|effect_builder| {
                let event = super::Event::CreatedFinalitySignature {
                    finality_signature: Box::new(fin_sig_2.clone()),
                };
                effect_builder
                    .into_inner()
                    .schedule(event, QueueKind::Validation)
                    .ignore()
            })
            .await;
        for _ in 0..6 {
            while runner.try_crank(&mut rng).await == TryCrankOutcome::NoEventsToProcess {
                time::sleep(POLL_INTERVAL).await;
            }
        }

        let expected_block = runner
            .reactor()
            .storage
            .read_block(block_2.hash())
            .unwrap()
            .unwrap();
        assert_eq!(expected_block, block_2);
        let expected_block_signatures = runner
            .reactor()
            .storage
            .get_finality_signatures_for_block(*block_2.hash());
        assert_eq!(
            expected_block_signatures
                .and_then(|sigs| sigs.get_finality_signature(&fin_sig_2.public_key))
                .unwrap(),
            fin_sig_2
        );
    }

    // Verify the state of the accumulator is correct.
    {
        let reactor = runner.reactor_mut();
        let block_accumulator = &mut reactor.block_accumulator;
        // Local tip should not have changed since no blocks were executed.
        assert_eq!(
            block_accumulator.local_tip,
            Some(LocalTipIdentifier::new(0, 0.into()))
        );

        assert!(!block_accumulator
            .block_acceptors
            .get(block_1.hash())
            .unwrap()
            .executed());
        assert!(block_accumulator
            .block_acceptors
            .get(block_1.hash())
            .unwrap()
            .has_sufficient_finality());
        assert_eq!(
            *block_accumulator
                .block_acceptors
                .get(block_1.hash())
                .unwrap()
                .peers(),
            BTreeSet::from([peer_1, peer_2])
        );

        assert!(!block_accumulator
            .block_acceptors
            .get(block_2.hash())
            .unwrap()
            .executed());
        assert!(block_accumulator
            .block_acceptors
            .get(block_2.hash())
            .unwrap()
            .has_sufficient_finality());
        assert_eq!(
            *block_accumulator
                .block_acceptors
                .get(block_2.hash())
                .unwrap()
                .peers(),
            BTreeSet::from([peer_2])
        );

        // Shouldn't have any complete blocks.
        assert!(runner
            .reactor()
            .storage
            .read_highest_complete_block()
            .unwrap()
            .is_none());
    }

    // Get the meta block along with the state, then register it as executed to
    // later notify the accumulator of its execution.
    let meta_block_1 = {
        let block_accumulator = &runner.reactor().block_accumulator;
        let mut meta_block = block_accumulator
            .block_acceptors
            .get(block_1.hash())
            .unwrap()
            .meta_block()
            .unwrap();
        assert!(meta_block.state.register_as_executed().was_updated());
        meta_block
    };

    // Let the accumulator know block 1 has been executed.
    {
        runner
            .process_injected_effects(|effect_builder| {
                let event = super::Event::ExecutedBlock {
                    meta_block: meta_block_1.clone(),
                };
                effect_builder
                    .into_inner()
                    .schedule(event, QueueKind::Validation)
                    .ignore()
            })
            .await;
        for _ in 0..4 {
            while runner.try_crank(&mut rng).await == TryCrankOutcome::NoEventsToProcess {
                time::sleep(POLL_INTERVAL).await;
            }
        }
    }

    // Verify the state of the accumulator is correct.
    {
        let reactor = runner.reactor_mut();
        let block_accumulator = &mut reactor.block_accumulator;
        // Local tip should now be block 1.
        let expected_local_tip =
            LocalTipIdentifier::new(block_1.header().height(), block_1.header().era_id());
        assert_eq!(block_accumulator.local_tip, Some(expected_local_tip));

        assert!(block_accumulator
            .block_acceptors
            .get(block_1.hash())
            .unwrap()
            .executed());
        assert!(block_accumulator
            .block_acceptors
            .get(block_1.hash())
            .unwrap()
            .has_sufficient_finality());
        assert_eq!(
            *block_accumulator
                .block_acceptors
                .get(block_1.hash())
                .unwrap()
                .peers(),
            BTreeSet::from([peer_1, peer_2])
        );
        // The block should be marked complete in storage by now.
        assert_eq!(
            runner
                .reactor()
                .storage
                .read_highest_complete_block()
                .unwrap()
                .unwrap()
                .height(),
            meta_block_1.block.height()
        );
    }

    // Retrigger the event so the accumulator can update its meta block state.
    {
        runner
            .process_injected_effects(|effect_builder| {
                let event = super::Event::ExecutedBlock {
                    meta_block: meta_block_1.clone(),
                };
                effect_builder
                    .into_inner()
                    .schedule(event, QueueKind::Validation)
                    .ignore()
            })
            .await;
        while runner.try_crank(&mut rng).await == TryCrankOutcome::NoEventsToProcess {
            time::sleep(POLL_INTERVAL).await;
        }
    }

    let older_block = generate_older_block(&mut rng, &block_1, 1);
    // Register an older block.
    {
        let effect_builder = runner.effect_builder();
        let reactor = runner.reactor_mut();

        let block_accumulator = &mut reactor.block_accumulator;
        let event = super::Event::ReceivedBlock {
            block: Arc::new(older_block.clone()),
            sender: peer_1,
        };
        let effects = block_accumulator.handle_event(effect_builder, &mut rng, event);
        assert!(effects.is_empty());
        // This should have no effect on the accumulator since the block is
        // older than the local tip.
        assert!(!block_accumulator
            .block_acceptors
            .contains_key(older_block.hash()));
    }

    let older_block_signature = FinalitySignature::create(
        *older_block.hash(),
        older_block.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    // Register a signature for an older block.
    {
        let effect_builder = runner.effect_builder();
        let reactor = runner.reactor_mut();

        let block_accumulator = &mut reactor.block_accumulator;
        let event = super::Event::ReceivedFinalitySignature {
            finality_signature: Box::new(older_block_signature),
            sender: peer_2,
        };
        let effects = block_accumulator.handle_event(effect_builder, &mut rng, event);
        assert!(effects.is_empty());
        // The block is older than the local tip, but the accumulator doesn't
        // know that because it was only provided with the signature, so it
        // creates the acceptor if it's in the same era or newer than the
        // local tip era, which, in this case, it is.
        assert!(block_accumulator
            .block_acceptors
            .contains_key(older_block.hash()));
    }

    let old_era_block = Block::random_with_specifics(
        &mut rng,
        block_1.header().era_id() - RECENT_ERA_INTERVAL - 1,
        1,
        ProtocolVersion::V1_0_0,
        false,
        None,
    );
    let old_era_signature = FinalitySignature::create(
        *old_era_block.hash(),
        old_era_block.header().era_id(),
        &ALICE_SECRET_KEY,
        ALICE_PUBLIC_KEY.clone(),
    );
    // Register a signature for a block in an old era.
    {
        let effect_builder = runner.effect_builder();
        let reactor = runner.reactor_mut();

        let block_accumulator = &mut reactor.block_accumulator;
        let event = super::Event::ReceivedFinalitySignature {
            finality_signature: Box::new(old_era_signature),
            sender: peer_2,
        };
        let effects = block_accumulator.handle_event(effect_builder, &mut rng, event);
        assert!(effects.is_empty());
        // This signature is from an older era and shouldn't lead to the
        // creation of an acceptor.
        assert!(!block_accumulator
            .block_acceptors
            .contains_key(old_era_block.hash()));
    }
}
