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
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error as ThisError;
use tokio::time;

use casper_types::{
    generate_ed25519_keypair, testing::TestRng, ActivationPoint, BlockV2, ChainNameDigest,
    Chainspec, ChainspecRawBytes, FinalitySignature, FinalitySignatureV2, ProtocolVersion,
    PublicKey, SecretKey, Signature, TestBlockBuilder, U512,
};
use reactor::ReactorEvent;

use crate::{
    components::{
        consensus::tests::utils::{
            ALICE_NODE_ID, ALICE_PUBLIC_KEY, ALICE_SECRET_KEY, BOB_NODE_ID, BOB_PUBLIC_KEY,
            BOB_SECRET_KEY, CAROL_PUBLIC_KEY, CAROL_SECRET_KEY,
        },
        network::Identity as NetworkIdentity,
        storage::{self, Storage},
    },
    effect::{
        announcements::ControlAnnouncement,
        requests::{ContractRuntimeRequest, MarkBlockCompletedRequest, NetworkRequest},
    },
    protocol::Message,
    reactor::{self, EventQueueHandle, QueueKind, Reactor, Runner, TryCrankOutcome},
    types::EraValidatorWeights,
    utils::{Loadable, WithDir},
    NodeRng,
};

use super::*;

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const RECENT_ERA_INTERVAL: u64 = 1;
const VALIDATOR_SLOTS: u32 = 100;

fn meta_block_with_default_state(block: Arc<BlockV2>) -> ForwardMetaBlock {
    MetaBlock::new_forward(block, vec![], MetaBlockState::new())
        .try_into()
        .unwrap()
}

fn signatures_for_block(
    block: &BlockV2,
    signatures: &[FinalitySignatureV2],
    chain_name_hash: ChainNameDigest,
) -> BlockSignaturesV2 {
    let mut block_signatures = BlockSignaturesV2::new(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
    );
    for signature in signatures {
        block_signatures.insert_signature(signature.public_key().clone(), *signature.signature());
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

impl From<MarkBlockCompletedRequest> for Event {
    fn from(request: MarkBlockCompletedRequest) -> Self {
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
        let (storage_config, storage_tempdir) = storage::Config::new_for_tests(1);
        let storage_withdir = WithDir::new(storage_tempdir.path(), storage_config);
        let validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
        let block_accumulator_config = Config::default();
        let block_time = block_accumulator_config.purge_interval / 2;

        let block_accumulator = BlockAccumulator::new(
            block_accumulator_config,
            validator_matrix.clone(),
            RECENT_ERA_INTERVAL,
            block_time,
            VALIDATOR_SLOTS,
            registry,
        )
        .unwrap();

        let storage = Storage::new(
            &storage_withdir,
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            EraId::default(),
            "test",
            chainspec.transaction_config.max_ttl.into(),
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
                let state = meta_block.mut_state();
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
                            super::Event::ExecutedBlock {
                                meta_block: meta_block.try_into().unwrap(),
                            },
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
    let block_time = config.purge_interval / 2;
    let metrics_registry = Registry::new();
    let mut accumulator = BlockAccumulator::new(
        config,
        validator_matrix,
        recent_era_interval,
        block_time,
        VALIDATOR_SLOTS,
        &metrics_registry,
    )
    .unwrap();

    let random_block_hash = BlockHash::random(&mut rng);
    accumulator.upsert_acceptor(random_block_hash, Some(era0), Some(*ALICE_NODE_ID));
    assert!(accumulator
        .block_acceptors
        .remove(&random_block_hash)
        .is_some());
    assert!(accumulator
        .peer_block_timestamps
        .remove(&ALICE_NODE_ID)
        .is_some());

    accumulator.register_local_tip(0, EraId::new(0));

    let max_block_count =
        PEER_RATE_LIMIT_MULTIPLIER * ((config.purge_interval / block_time) as usize);

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
    let purge_interval = config.purge_interval * 2;
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
    let block = TestBlockBuilder::new().build(&mut rng);
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
    let rng = &mut TestRng::new();
    // Create a block and an acceptor for it.
    let block = Arc::new(TestBlockBuilder::new().build(rng));
    let chain_name_hash = ChainNameDigest::random(rng);
    let mut meta_block: ForwardMetaBlock =
        MetaBlock::new_forward(block.clone(), vec![], MetaBlockState::new())
            .try_into()
            .unwrap();
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);

    // Create a finality signature with the wrong block hash.
    let wrong_fin_sig = FinalitySignatureV2::random_for_block(
        BlockHash::random(rng),
        rng.gen(),
        EraId::new(0),
        ChainNameDigest::random(rng),
        rng,
    );
    assert!(matches!(
        acceptor
            .register_finality_signature(wrong_fin_sig, None, VALIDATOR_SLOTS)
            .unwrap_err(),
        Error::BlockHashMismatch {
            expected: _,
            actual: _
        }
    ));

    // Create an invalid finality signature.
    let invalid_fin_sig = FinalitySignatureV2::new(
        *block.hash(),
        block.height(),
        EraId::random(rng),
        chain_name_hash,
        Signature::System,
        PublicKey::random(rng),
    );
    // We shouldn't be able to create invalid signatures ourselves, so we've
    // reached an invalid state.
    assert!(matches!(
        acceptor
            .register_finality_signature(invalid_fin_sig.clone(), None, VALIDATOR_SLOTS)
            .unwrap_err(),
        Error::InvalidConfiguration
    ));
    // Peers shouldn't send us invalid signatures.
    let first_peer = NodeId::random(rng);
    assert!(matches!(
        acceptor
            .register_finality_signature(invalid_fin_sig, Some(first_peer), VALIDATOR_SLOTS)
            .unwrap_err(),
        Error::InvalidGossip(_)
    ));
    // Create a valid finality signature and register it.
    let fin_sig = FinalitySignatureV2::random_for_block(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        rng,
    );
    assert!(acceptor
        .register_finality_signature(fin_sig.clone(), Some(first_peer), VALIDATOR_SLOTS)
        .unwrap()
        .is_none());
    // Register it from the second peer as well.
    let second_peer = NodeId::random(rng);
    assert!(acceptor
        .register_finality_signature(fin_sig.clone(), Some(second_peer), VALIDATOR_SLOTS)
        .unwrap()
        .is_none());
    // Make sure the peer list is updated accordingly.
    let (sig, senders) = acceptor.signatures().get(fin_sig.public_key()).unwrap();
    assert_eq!(*sig, fin_sig);
    assert_eq!(*senders, BTreeSet::from([first_peer, second_peer]));
    // Create a second finality signature and register it.
    let second_fin_sig = FinalitySignatureV2::random_for_block(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        rng,
    );
    assert!(acceptor
        .register_finality_signature(second_fin_sig.clone(), Some(first_peer), VALIDATOR_SLOTS)
        .unwrap()
        .is_none());
    // Make sure the peer list for the first signature is unchanged.
    let (first_sig, first_sig_senders) = acceptor.signatures().get(fin_sig.public_key()).unwrap();
    assert_eq!(*first_sig, fin_sig);
    assert_eq!(
        *first_sig_senders,
        BTreeSet::from([first_peer, second_peer])
    );
    // Make sure the peer list for the second signature is correct.
    let (sig, senders) = acceptor
        .signatures()
        .get(second_fin_sig.public_key())
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
    let wrong_era = EraId::from(u64::MAX ^ u64::from(block.era_id()));
    let invalid_fin_sig = FinalitySignatureV2::random_for_block(
        *block.hash(),
        block.height(),
        wrong_era,
        chain_name_hash,
        rng,
    );
    assert!(matches!(
        acceptor
            .register_finality_signature(invalid_fin_sig.clone(), Some(first_peer), VALIDATOR_SLOTS)
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
            .register_finality_signature(invalid_fin_sig, None, VALIDATOR_SLOTS)
            .unwrap_err(),
        Error::InvalidConfiguration
    ));
    // Registering valid signatures still works, but we already had the second
    // signature.
    assert!(acceptor
        .register_finality_signature(second_fin_sig.clone(), Some(second_peer), VALIDATOR_SLOTS)
        .unwrap()
        .is_none());
    assert!(acceptor
        .signatures()
        .get(second_fin_sig.public_key())
        .unwrap()
        .1
        .contains(&second_peer));
    // Register a new valid signature which should be yielded by the function.
    let third_fin_sig = FinalitySignatureV2::random_for_block(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        rng,
    );
    assert_eq!(
        acceptor
            .register_finality_signature(third_fin_sig.clone(), Some(first_peer), VALIDATOR_SLOTS)
            .unwrap()
            .unwrap(),
        third_fin_sig
    );
    // Additional registrations of the third signature with and without a peer
    // should still work.
    assert!(acceptor
        .register_finality_signature(third_fin_sig.clone(), Some(second_peer), VALIDATOR_SLOTS)
        .unwrap()
        .is_none());
    assert!(acceptor
        .register_finality_signature(third_fin_sig, None, VALIDATOR_SLOTS)
        .unwrap()
        .is_none());
}

#[test]
fn acceptor_register_block() {
    let mut rng = TestRng::new();
    // Create a block and an acceptor for it.
    let block = Arc::new(TestBlockBuilder::new().build(&mut rng));
    let mut meta_block = meta_block_with_default_state(block.clone());
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);

    // Create a finality signature with the wrong block hash.
    let wrong_block =
        meta_block_with_default_state(Arc::new(TestBlockBuilder::new().build(&mut rng)));
    assert!(matches!(
        acceptor.register_block(wrong_block, None).unwrap_err(),
        Error::BlockHashMismatch {
            expected: _,
            actual: _
        }
    ));

    {
        // Invalid block case.
        let invalid_block: Arc<BlockV2> = Arc::new(TestBlockBuilder::new().build_invalid(&mut rng));

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
    let chain_name_hash = ChainNameDigest::random(&mut rng);
    let block = Arc::new(TestBlockBuilder::new().build(&mut rng));
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
        block.era_id(),
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
    let (should_store, _offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);
    // Reset the flag.
    acceptor.set_sufficient_finality(false);

    let (should_store, offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);
    assert!(offenders.is_empty());

    let mut signatures = vec![];

    // Create the first validator's signature.
    let fin_sig = FinalitySignatureV2::create(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        &keys[0].0,
    );
    signatures.push(fin_sig.clone());
    // First signature with 40% weight brings the block to weak finality.
    acceptor
        .register_finality_signature(fin_sig, None, VALIDATOR_SLOTS)
        .unwrap();
    let (should_store, _offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Registering the block now.
    acceptor.register_block(meta_block.clone(), None).unwrap();
    let (should_store, _offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Create the third validator's signature.
    let fin_sig = FinalitySignatureV2::create(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        &keys[2].0,
    );
    // The third signature with weight 10% doesn't make the block go to
    // strict finality.
    signatures.push(fin_sig.clone());
    acceptor
        .register_finality_signature(fin_sig, None, VALIDATOR_SLOTS)
        .unwrap();
    let (should_store, _offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Create a bogus signature from a non-validator for this era.
    let non_validator_keys = generate_ed25519_keypair();
    let faulty_peer = NodeId::random(&mut rng);
    let bogus_sig = FinalitySignatureV2::create(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        &non_validator_keys.0,
    );
    acceptor
        .register_finality_signature(bogus_sig, Some(faulty_peer), VALIDATOR_SLOTS)
        .unwrap();
    let (should_store, offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);
    // Make sure the peer who sent us this bogus signature is marked as an
    // offender.
    assert_eq!(offenders[0].0, faulty_peer);

    // Create the second validator's signature.
    let fin_sig = FinalitySignatureV2::create(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        &keys[1].0,
    );
    signatures.push(fin_sig.clone());
    // Second signature with 40% weight brings the block to strict finality.
    acceptor
        .register_finality_signature(fin_sig, None, VALIDATOR_SLOTS)
        .unwrap();
    let (should_store, _offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    let block_signatures = signatures_for_block(&block, &signatures, chain_name_hash);
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
    let fin_sig = FinalitySignatureV2::create(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        &keys[3].0,
    );
    // Already have sufficient finality signatures, so we're not supposed to
    // store anything else.
    acceptor
        .register_finality_signature(fin_sig, None, VALIDATOR_SLOTS)
        .unwrap();
    let (should_store, _offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Without the block, even with sufficient signatures we should not store anything.
    acceptor.set_meta_block(None);
    acceptor.set_sufficient_finality(false);
    let (should_store, _offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);

    // Without any signatures, we should not store anything.
    meta_block.state.register_has_sufficient_finality();
    acceptor.set_meta_block(Some(meta_block));
    acceptor.signatures_mut().retain(|_, _| false);
    let (should_store, _offenders) =
        acceptor.should_store_block(&era_validator_weights, chain_name_hash);
    assert_eq!(should_store, ShouldStore::Nothing);
}

#[test]
fn acceptor_should_correctly_bound_the_signatures() {
    let mut rng = TestRng::new();
    let validator_slots = 2;

    // Create a block and an acceptor for it.
    let block = Arc::new(TestBlockBuilder::new().build(&mut rng));
    let chain_name_hash = ChainNameDigest::random(&mut rng);
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);
    let first_peer = NodeId::random(&mut rng);

    // Fill the signatures map:
    for fin_sig in (0..validator_slots * 2).map(|_| {
        FinalitySignatureV2::random_for_block(
            *block.hash(),
            block.height(),
            block.era_id(),
            chain_name_hash,
            &mut rng,
        )
    }) {
        assert!(acceptor
            .register_finality_signature(fin_sig, Some(first_peer), validator_slots)
            .unwrap()
            .is_none());
    }

    let fin_sig = FinalitySignatureV2::random_for_block(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        &mut rng,
    );
    assert!(matches!(
        acceptor.register_finality_signature(fin_sig, Some(first_peer), validator_slots),
        Err(Error::TooManySignatures { .. }),
    ));
}

#[test]
fn acceptor_signatures_bound_should_not_be_triggered_if_peers_are_different() {
    let mut rng = TestRng::new();
    let validator_slots = 3;

    // Create a block and an acceptor for it.
    let block = Arc::new(TestBlockBuilder::new().build(&mut rng));
    let chain_name_hash = ChainNameDigest::random(&mut rng);
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);
    let first_peer = NodeId::random(&mut rng);
    let second_peer = NodeId::random(&mut rng);

    // Fill the signatures map:
    for fin_sig in (0..validator_slots).map(|_| {
        FinalitySignatureV2::random_for_block(
            *block.hash(),
            block.height(),
            block.era_id(),
            chain_name_hash,
            &mut rng,
        )
    }) {
        assert!(acceptor
            .register_finality_signature(fin_sig, Some(first_peer), validator_slots)
            .unwrap()
            .is_none());
    }

    // This should pass, because it is another peer:
    let fin_sig = FinalitySignatureV2::random_for_block(
        *block.hash(),
        block.height(),
        block.era_id(),
        chain_name_hash,
        &mut rng,
    );
    assert!(acceptor
        .register_finality_signature(fin_sig, Some(second_peer), validator_slots)
        .unwrap()
        .is_none());
}

#[test]
fn accumulator_should_leap() {
    let mut rng = TestRng::new();
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    let block_accumulator_config = Config::default();
    let recent_era_interval = 1;
    let block_time = block_accumulator_config.purge_interval / 2;
    let attempt_execution_threshold = block_accumulator_config.attempt_execution_threshold;
    let mut block_accumulator = BlockAccumulator::new(
        block_accumulator_config,
        validator_matrix.clone(),
        recent_era_interval,
        block_time,
        VALIDATOR_SLOTS,
        &Registry::default(),
    )
    .unwrap();

    let era_id = EraId::from(0);
    let chain_name_hash = ChainNameDigest::random(&mut rng);

    // Register the era in the validator matrix so the block is valid.
    register_evw_for_era(&mut validator_matrix, era_id);

    assert!(
        block_accumulator.local_tip.is_none(),
        "block_accumulator local tip should init null"
    );

    expected_leap_instruction(
        LeapInstruction::UnsetLocalTip,
        block_accumulator.leap_instruction(&SyncIdentifier::BlockIdentifier(
            BlockHash::random(&mut rng),
            0,
        )),
    );

    block_accumulator.local_tip = Some(LocalTipIdentifier::new(1, era_id));

    let synced = SyncIdentifier::BlockHash(BlockHash::random(&mut rng));
    expected_leap_instruction(
        LeapInstruction::UnknownBlockHeight,
        block_accumulator.leap_instruction(&synced),
    );

    let synced = SyncIdentifier::SyncedBlockIdentifier(BlockHash::random(&mut rng), 1, era_id);
    expected_leap_instruction(
        LeapInstruction::NoUsableBlockAcceptors,
        block_accumulator.leap_instruction(&synced),
    );

    // Create an acceptor to change the highest usable block height.
    {
        let block = TestBlockBuilder::new()
            .era(era_id)
            .height(1)
            .switch_block(false)
            .build(&mut rng);

        block_accumulator
            .block_acceptors
            .insert(*block.hash(), block_acceptor(block, chain_name_hash));
    }

    expected_leap_instruction(
        LeapInstruction::AtHighestKnownBlock,
        block_accumulator.leap_instruction(&synced),
    );

    let block_height = attempt_execution_threshold;
    // Insert an acceptor within execution range
    {
        let block = TestBlockBuilder::new()
            .era(era_id)
            .height(block_height)
            .switch_block(false)
            .build(&mut rng);

        block_accumulator
            .block_acceptors
            .insert(*block.hash(), block_acceptor(block, chain_name_hash));
    }

    expected_leap_instruction(
        LeapInstruction::WithinAttemptExecutionThreshold(
            attempt_execution_threshold.saturating_sub(1),
        ),
        block_accumulator.leap_instruction(&synced),
    );

    let centurion = 100;
    // Insert an upgrade boundary
    {
        let block = TestBlockBuilder::new()
            .era(era_id)
            .height(centurion)
            .switch_block(true)
            .build(&mut rng);

        block_accumulator
            .block_acceptors
            .insert(*block.hash(), block_acceptor(block, chain_name_hash));
    }

    expected_leap_instruction(
        LeapInstruction::AtHighestKnownBlock,
        block_accumulator.leap_instruction(&SyncIdentifier::SyncedBlockIdentifier(
            BlockHash::random(&mut rng),
            centurion,
            era_id,
        )),
    );
    expected_leap_instruction(
        LeapInstruction::OutsideAttemptExecutionThreshold(attempt_execution_threshold + 1),
        block_accumulator.leap_instruction(&SyncIdentifier::SyncedBlockIdentifier(
            BlockHash::random(&mut rng),
            centurion - attempt_execution_threshold - 1,
            era_id,
        )),
    );

    let offset = centurion.saturating_sub(attempt_execution_threshold);
    for height in offset..centurion {
        expected_leap_instruction(
            LeapInstruction::WithinAttemptExecutionThreshold(centurion.saturating_sub(height)),
            block_accumulator.leap_instruction(&SyncIdentifier::SyncedBlockIdentifier(
                BlockHash::random(&mut rng),
                height,
                era_id,
            )),
        );
    }

    let upgrade_attempt_execution_threshold = attempt_execution_threshold * 2;
    block_accumulator.register_activation_point(ActivationPoint::EraId(era_id.successor()));
    let offset = centurion.saturating_sub(upgrade_attempt_execution_threshold);
    for height in offset..centurion {
        expected_leap_instruction(
            LeapInstruction::TooCloseToUpgradeBoundary(centurion.saturating_sub(height)),
            block_accumulator.leap_instruction(&SyncIdentifier::SyncedBlockIdentifier(
                BlockHash::random(&mut rng),
                height,
                era_id,
            )),
        );
    }

    expected_leap_instruction(
        LeapInstruction::AtHighestKnownBlock,
        block_accumulator.leap_instruction(&SyncIdentifier::SyncedBlockIdentifier(
            BlockHash::random(&mut rng),
            centurion,
            era_id,
        )),
    );
    expected_leap_instruction(
        LeapInstruction::OutsideAttemptExecutionThreshold(upgrade_attempt_execution_threshold + 1),
        block_accumulator.leap_instruction(&SyncIdentifier::SyncedBlockIdentifier(
            BlockHash::random(&mut rng),
            centurion - upgrade_attempt_execution_threshold - 1,
            era_id,
        )),
    );
}

fn expected_leap_instruction(expected: LeapInstruction, actual: LeapInstruction) {
    assert!(
        expected.eq(&actual),
        "{}",
        format!("expected: {} actual: {}", expected, actual)
    );
}

fn block_acceptor(block: BlockV2, chain_name_hash: ChainNameDigest) -> BlockAcceptor {
    let mut acceptor = BlockAcceptor::new(*block.hash(), vec![]);
    // One finality signature from our only validator for block 1.
    acceptor
        .register_finality_signature(
            FinalitySignatureV2::create(
                *block.hash(),
                block.height(),
                block.era_id(),
                chain_name_hash,
                &ALICE_SECRET_KEY,
            ),
            None,
            VALIDATOR_SLOTS,
        )
        .unwrap();

    let meta_block = {
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        MetaBlock::new_forward(Arc::new(block), vec![], state)
            .try_into()
            .unwrap()
    };
    acceptor.register_block(meta_block, None).unwrap();

    acceptor
}

#[test]
fn accumulator_purge() {
    let mut rng = TestRng::new();
    let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    let block_accumulator_config = Config::default();
    let recent_era_interval = 1;
    let block_time = block_accumulator_config.purge_interval / 2;
    let purge_interval = block_accumulator_config.purge_interval;
    let time_before_insertion = Timestamp::now();
    let mut block_accumulator = BlockAccumulator::new(
        block_accumulator_config,
        validator_matrix.clone(),
        recent_era_interval,
        block_time,
        VALIDATOR_SLOTS,
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

    let chain_name_hash = ChainNameDigest::random(&mut rng);

    // One finality signature from our only validator for block 1.
    let fin_sig_1 = FinalitySignatureV2::create(
        *block_1.hash(),
        block_1.height(),
        block_1.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );
    // One finality signature from our only validator for block 2.
    let fin_sig_2 = FinalitySignatureV2::create(
        *block_2.hash(),
        block_2.height(),
        block_2.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );
    // One finality signature from our only validator for block 3.
    let fin_sig_3 = FinalitySignatureV2::create(
        *block_3.hash(),
        block_3.height(),
        block_3.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );

    // Register the eras in the validator matrix so the blocks are valid.
    {
        register_evw_for_era(&mut validator_matrix, block_1.era_id());
        register_evw_for_era(&mut validator_matrix, block_2.era_id());
        register_evw_for_era(&mut validator_matrix, block_3.era_id());
    }

    // We will manually call `upsert_acceptor` in order to have
    // `peer_block_timestamps` populated.
    {
        // Insert the first block with sufficient finality from the first peer.
        block_accumulator.upsert_acceptor(*block_1.hash(), Some(block_1.era_id()), Some(peer_1));
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(block_1.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new_forward(block_1.clone(), vec![], state)
            .try_into()
            .unwrap();
        acceptor
            .register_finality_signature(fin_sig_1, Some(peer_1), VALIDATOR_SLOTS)
            .unwrap();
        acceptor.register_block(meta_block, None).unwrap();
    }

    {
        // Insert the second block with sufficient finality from the second
        // peer.
        block_accumulator.upsert_acceptor(*block_2.hash(), Some(block_2.era_id()), Some(peer_2));
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(block_2.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new_forward(block_2.clone(), vec![], state)
            .try_into()
            .unwrap();
        acceptor
            .register_finality_signature(fin_sig_2, Some(peer_2), VALIDATOR_SLOTS)
            .unwrap();
        acceptor.register_block(meta_block, None).unwrap();
    }

    {
        // Insert the third block with sufficient finality from the third peer.
        block_accumulator.upsert_acceptor(*block_3.hash(), Some(block_3.era_id()), Some(peer_1));
        block_accumulator.upsert_acceptor(*block_3.hash(), Some(block_3.era_id()), Some(peer_2));
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(block_3.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new_forward(block_3.clone(), vec![], state)
            .try_into()
            .unwrap();
        acceptor
            .register_finality_signature(fin_sig_3, Some(peer_1), VALIDATOR_SLOTS)
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
    // Acceptors for blocks 1 and 2 should not have been purged because they
    // have strict finality.
    assert!(block_accumulator
        .block_acceptors
        .contains_key(block_1.hash()));
    assert!(block_accumulator
        .block_acceptors
        .contains_key(block_2.hash()));
    assert!(block_accumulator
        .block_acceptors
        .contains_key(block_3.hash()));
    // We should have kept only the timestamps for the first peer.
    assert!(block_accumulator
        .peer_block_timestamps
        .contains_key(&peer_1));
    assert!(!block_accumulator
        .peer_block_timestamps
        .contains_key(&peer_2));

    {
        // Modify the `strict_finality` flag in the acceptors for blocks 1 and
        // 2.
        block_accumulator
            .block_acceptors
            .get_mut(block_1.hash())
            .unwrap()
            .set_sufficient_finality(false);
        block_accumulator
            .block_acceptors
            .get_mut(block_2.hash())
            .unwrap()
            .set_sufficient_finality(false);
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

    // Create a block just in range of block 3 to not qualify for a purge.
    let in_range_block = Arc::new(
        TestBlockBuilder::new()
            .era(block_3.era_id())
            .height(block_3.height() - block_accumulator.attempt_execution_threshold)
            .protocol_version(block_3.protocol_version())
            .switch_block(false)
            .build(&mut rng),
    );

    let in_range_block_sig = FinalitySignatureV2::create(
        *in_range_block.hash(),
        in_range_block.height(),
        in_range_block.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );

    {
        // Insert the in range block with sufficient finality.
        block_accumulator.upsert_acceptor(
            *in_range_block.hash(),
            Some(in_range_block.era_id()),
            Some(peer_1),
        );
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(in_range_block.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new_forward(in_range_block.clone(), vec![], state)
            .try_into()
            .unwrap();
        acceptor
            .register_finality_signature(in_range_block_sig, Some(peer_1), VALIDATOR_SLOTS)
            .unwrap();
        acceptor.register_block(meta_block, Some(peer_2)).unwrap();
    }

    // Create a block just out of range of block 3 to qualify for a purge.
    let out_of_range_block = Arc::new(
        TestBlockBuilder::new()
            .era(block_3.era_id())
            .height(block_3.height() - block_accumulator.attempt_execution_threshold - 1)
            .protocol_version(block_3.protocol_version())
            .switch_block(false)
            .build(&mut rng),
    );
    let out_of_range_block_sig = FinalitySignatureV2::create(
        *out_of_range_block.hash(),
        out_of_range_block.height(),
        out_of_range_block.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );

    {
        // Insert the out of range block with sufficient finality.
        block_accumulator.upsert_acceptor(
            *out_of_range_block.hash(),
            Some(out_of_range_block.era_id()),
            Some(peer_1),
        );
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(out_of_range_block.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new_forward(out_of_range_block.clone(), vec![], state)
            .try_into()
            .unwrap();
        acceptor
            .register_finality_signature(out_of_range_block_sig, Some(peer_1), VALIDATOR_SLOTS)
            .unwrap();
        acceptor.register_block(meta_block, Some(peer_2)).unwrap();
    }

    // Make sure the local tip along with its recent parents never get purged.
    {
        assert!(block_accumulator
            .block_acceptors
            .contains_key(block_3.hash()));
        // Make block 3 the local tip.
        block_accumulator.local_tip =
            Some(LocalTipIdentifier::new(block_3.height(), block_3.era_id()));
        // Change the timestamps to old ones so that all blocks would normally
        // get purged.
        let last_progress = time_before_insertion.saturating_sub(purge_interval * 10);
        for (_, acceptor) in block_accumulator.block_acceptors.iter_mut() {
            acceptor.set_last_progress(last_progress);
        }
        for (_, timestamps) in block_accumulator.peer_block_timestamps.iter_mut() {
            for (_, timestamp) in timestamps.iter_mut() {
                *timestamp = last_progress;
            }
        }
        // Do the purge.
        block_accumulator.purge();
        // As block 3 is the local tip, it should not have been purged.
        assert!(block_accumulator
            .block_acceptors
            .contains_key(block_3.hash()));
        // Neither should the block in `attempt_execution_threshold` range.
        assert!(block_accumulator
            .block_acceptors
            .contains_key(in_range_block.hash()));
        // But the block out of `attempt_execution_threshold` range should
        // have been purged.
        assert!(!block_accumulator
            .block_acceptors
            .contains_key(out_of_range_block.hash()));

        // Now replace the local tip with something else (in this case we'll
        // have no local tip) so that previously created blocks no longer have
        // purge immunity.
        block_accumulator.local_tip.take();
        // Do the purge.
        block_accumulator.purge();
        // Block 3 is no longer the local tip, and given that it's old, the
        // blocks should have been purged.
        assert!(block_accumulator.block_acceptors.is_empty());
    }

    // Create a future block after block 3.
    let future_block = Arc::new(
        TestBlockBuilder::new()
            .era(block_3.era_id())
            .height(block_3.height() + block_accumulator.attempt_execution_threshold)
            .protocol_version(block_3.protocol_version())
            .switch_block(false)
            .build(&mut rng),
    );
    let future_block_sig = FinalitySignatureV2::create(
        *future_block.hash(),
        future_block.height(),
        future_block.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );

    {
        // Insert the future block with sufficient finality.
        block_accumulator.upsert_acceptor(
            *future_block.hash(),
            Some(future_block.era_id()),
            Some(peer_1),
        );
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(future_block.hash())
            .unwrap();
        let mut state = MetaBlockState::new();
        state.register_has_sufficient_finality();
        let meta_block = MetaBlock::new_forward(future_block.clone(), vec![], state)
            .try_into()
            .unwrap();
        acceptor
            .register_finality_signature(future_block_sig, Some(peer_1), VALIDATOR_SLOTS)
            .unwrap();
        acceptor.register_block(meta_block, Some(peer_2)).unwrap();
    }

    // Create a future block after block 3, but which will not have strict
    // finality.
    let future_unsigned_block = Arc::new(
        TestBlockBuilder::new()
            .era(block_3.era_id())
            .height(block_3.height() + block_accumulator.attempt_execution_threshold * 2)
            .protocol_version(block_3.protocol_version())
            .switch_block(false)
            .build(&mut rng),
    );
    let future_unsigned_block_sig = FinalitySignatureV2::create(
        *future_unsigned_block.hash(),
        future_unsigned_block.height(),
        future_unsigned_block.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );

    {
        // Insert the future unsigned block without sufficient finality.
        block_accumulator.upsert_acceptor(
            *future_unsigned_block.hash(),
            Some(future_unsigned_block.era_id()),
            Some(peer_1),
        );
        let acceptor = block_accumulator
            .block_acceptors
            .get_mut(future_unsigned_block.hash())
            .unwrap();
        let state = MetaBlockState::new();
        let meta_block = MetaBlock::new_forward(future_unsigned_block.clone(), vec![], state)
            .try_into()
            .unwrap();
        acceptor
            .register_finality_signature(future_unsigned_block_sig, Some(peer_1), VALIDATOR_SLOTS)
            .unwrap();
        acceptor.register_block(meta_block, Some(peer_2)).unwrap();
    }

    // Make sure block with sufficient finality doesn't get purged.
    {
        // Make block 3 the local tip again.
        block_accumulator.local_tip =
            Some(LocalTipIdentifier::new(block_3.height(), block_3.era_id()));
        assert!(block_accumulator
            .block_acceptors
            .contains_key(future_block.hash()));
        assert!(block_accumulator
            .block_acceptors
            .contains_key(future_unsigned_block.hash()));

        // Change the timestamps to old ones so that all blocks would normally
        // get purged.
        let last_progress = time_before_insertion.saturating_sub(purge_interval * 10);
        for (_, acceptor) in block_accumulator.block_acceptors.iter_mut() {
            acceptor.set_last_progress(last_progress);
        }
        for (_, timestamps) in block_accumulator.peer_block_timestamps.iter_mut() {
            for (_, timestamp) in timestamps.iter_mut() {
                *timestamp = last_progress;
            }
        }
        // Do the purge.
        block_accumulator.purge();
        // Neither should the future block with sufficient finality.
        assert!(block_accumulator
            .block_acceptors
            .contains_key(future_block.hash()));
        // But the future block without sufficient finality should have been
        // purged.
        assert!(!block_accumulator
            .block_acceptors
            .contains_key(future_unsigned_block.hash()));

        // Now replace the local tip with something else (in this case we'll
        // have no local tip) so that previously created blocks no longer have
        // purge immunity.
        block_accumulator.local_tip.take();
        // Do the purge.
        block_accumulator.purge();
        // Block 3 is no longer the local tip, and given that it's old, the
        // blocks should have been purged.
        assert!(block_accumulator.block_acceptors.is_empty());
    }
}

fn register_evw_for_era(validator_matrix: &mut ValidatorMatrix, era_id: EraId) {
    let weights = EraValidatorWeights::new(
        era_id,
        BTreeMap::from([(ALICE_PUBLIC_KEY.clone(), 100.into())]),
        Ratio::new(1, 3),
    );
    validator_matrix.register_era_validator_weights(weights);
}

fn generate_next_block(rng: &mut TestRng, block: &BlockV2) -> BlockV2 {
    let era_id = if block.is_switch_block() {
        block.era_id().successor()
    } else {
        block.era_id()
    };

    TestBlockBuilder::new()
        .era(era_id)
        .height(block.height() + 1)
        .protocol_version(block.protocol_version())
        .switch_block(false)
        .build(rng)
}

fn generate_non_genesis_block(rng: &mut TestRng) -> BlockV2 {
    let era = rng.gen_range(10..20);
    let height = era * 10 + rng.gen_range(0..10);
    let is_switch = rng.gen_bool(0.1);

    TestBlockBuilder::new()
        .era(era)
        .height(height)
        .switch_block(is_switch)
        .build(rng)
}

fn generate_older_block(rng: &mut TestRng, block: &BlockV2, height_difference: u64) -> BlockV2 {
    TestBlockBuilder::new()
        .era(block.era_id().predecessor().unwrap_or_default())
        .height(block.height() - height_difference)
        .protocol_version(block.protocol_version())
        .switch_block(false)
        .build(rng)
}

#[tokio::test]
async fn block_accumulator_reactor_flow() {
    let mut rng = TestRng::new();
    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let chain_name_hash = chainspec.name_hash();
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
    let fin_sig_1 = FinalitySignatureV2::create(
        *block_1.hash(),
        block_1.height(),
        block_1.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );
    // One finality signature from our only validator for block 2.
    let fin_sig_2 = FinalitySignatureV2::create(
        *block_2.hash(),
        block_2.height(),
        block_2.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );

    // Register the eras in the validator matrix so the blocks are valid.
    {
        let mut validator_matrix = runner.reactor_mut().validator_matrix.clone();
        register_evw_for_era(&mut validator_matrix, block_1.era_id());
        register_evw_for_era(&mut validator_matrix, block_2.era_id());
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
        assert_eq!(expected_block, block_1.clone().into());
        let expected_block_signatures = runner
            .reactor()
            .storage
            .get_finality_signatures_for_block(*block_1.hash());
        assert_eq!(
            expected_block_signatures
                .and_then(|sigs| sigs.finality_signature(fin_sig_1.public_key()))
                .unwrap(),
            FinalitySignature::from(fin_sig_1)
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
        assert_eq!(expected_block, block_2.clone().into());
        let expected_block_signatures = runner
            .reactor()
            .storage
            .get_finality_signatures_for_block(*block_2.hash());
        assert_eq!(
            expected_block_signatures
                .and_then(|sigs| sigs.finality_signature(fin_sig_2.public_key()))
                .unwrap(),
            FinalitySignature::from(fin_sig_2)
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
        let expected_local_tip = LocalTipIdentifier::new(block_1.height(), block_1.era_id());
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

    let older_block_signature = FinalitySignatureV2::create(
        *older_block.hash(),
        older_block.height(),
        older_block.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
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

    let old_era_block = TestBlockBuilder::new()
        .era(block_1.era_id() - RECENT_ERA_INTERVAL - 1)
        .height(1)
        .switch_block(false)
        .build(&mut rng);

    let old_era_signature = FinalitySignatureV2::create(
        *old_era_block.hash(),
        old_era_block.height(),
        old_era_block.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
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

#[tokio::test]
async fn block_accumulator_doesnt_purge_with_delayed_block_execution() {
    let mut rng = TestRng::new();
    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let chain_name_hash = chainspec.name_hash();
    let mut runner: Runner<MockReactor> = Runner::new(
        (),
        Arc::new(chainspec),
        Arc::new(chainspec_raw_bytes),
        &mut rng,
    )
    .await
    .unwrap();

    // Create 1 block.
    let block_1 = generate_non_genesis_block(&mut rng);

    // Also create 2 peers.
    let peer_1 = NodeId::random(&mut rng);
    let peer_2 = NodeId::random(&mut rng);

    let fin_sig_bob = FinalitySignatureV2::create(
        *block_1.hash(),
        block_1.height(),
        block_1.era_id(),
        chain_name_hash,
        &BOB_SECRET_KEY,
    );

    let fin_sig_carol = FinalitySignatureV2::create(
        *block_1.hash(),
        block_1.height(),
        block_1.era_id(),
        chain_name_hash,
        &CAROL_SECRET_KEY,
    );

    let fin_sig_alice = FinalitySignatureV2::create(
        *block_1.hash(),
        block_1.height(),
        block_1.era_id(),
        chain_name_hash,
        &ALICE_SECRET_KEY,
    );

    // Register the era in the validator matrix so the block is valid.
    {
        let mut validator_matrix = runner.reactor_mut().validator_matrix.clone();
        let weights = EraValidatorWeights::new(
            block_1.era_id(),
            BTreeMap::from([
                (ALICE_PUBLIC_KEY.clone(), 10.into()), /* Less weight so that the sig from Alice
                                                        * would not have sufficient finality */
                (BOB_PUBLIC_KEY.clone(), 100.into()),
                (CAROL_PUBLIC_KEY.clone(), 100.into()),
            ]),
            Ratio::new(1, 3),
        );
        validator_matrix.register_era_validator_weights(weights);
    }

    // Register signatures for block 1.
    {
        let effect_builder = runner.effect_builder();
        let reactor = runner.reactor_mut();

        let block_accumulator = &mut reactor.block_accumulator;
        block_accumulator.register_local_tip(0, 0.into());

        let event = super::Event::ReceivedFinalitySignature {
            finality_signature: Box::new(fin_sig_bob.clone()),
            sender: peer_1,
        };
        let effects = block_accumulator.handle_event(effect_builder, &mut rng, event);
        assert!(effects.is_empty());

        let event = super::Event::ReceivedFinalitySignature {
            finality_signature: Box::new(fin_sig_carol.clone()),
            sender: peer_1,
        };
        let effects = block_accumulator.handle_event(effect_builder, &mut rng, event);
        assert!(effects.is_empty());

        // Register the finality signature created by Alice (this validator) after executing the
        // block.
        let event = super::Event::CreatedFinalitySignature {
            finality_signature: Box::new(fin_sig_alice.clone()),
        };
        let effects = block_accumulator.handle_event(effect_builder, &mut rng, event);
        assert!(effects.is_empty());
    }

    // Register block 1 as received from peer.
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
        assert_eq!(expected_block, block_1.clone().into());
        let expected_block_signatures = runner
            .reactor()
            .storage
            .get_finality_signatures_for_block(*block_1.hash());
        assert_eq!(
            expected_block_signatures
                .and_then(|sigs| sigs.finality_signature(fin_sig_alice.public_key()))
                .unwrap(),
            FinalitySignature::from(fin_sig_alice)
        );
    }

    // Now add a delay between when the finality signature is created and registered in the
    // accumulator. Usually registering the created finality signature and the executed block
    // happen immediately but if the event queue is backed up the event to register the executed
    // block can be delayed. Since we would purge an acceptor if the purge interval has passed,
    // we want to simulate a situation in which the purge interval was exceeded in order to test
    // the special case that if an acceptor that had sufficient finality, it is not purged.
    tokio::time::sleep(
        Duration::from(runner.reactor().block_accumulator.purge_interval) + Duration::from_secs(1),
    )
    .await;

    // Register block 1 as having been executed by Alice (this node).
    {
        runner
            .process_injected_effects(|effect_builder| {
                let mut meta_block_state = MetaBlockState::new_already_stored();
                meta_block_state.register_as_executed();
                let event = super::Event::ExecutedBlock {
                    meta_block: MetaBlock::new_forward(
                        Arc::new(block_1.clone()),
                        Vec::new(),
                        meta_block_state,
                    )
                    .try_into()
                    .unwrap(),
                };
                effect_builder
                    .into_inner()
                    .schedule(event, QueueKind::Regular)
                    .ignore()
            })
            .await;
        let mut finished = false;
        while !finished {
            let mut retry_count = 5;
            while runner.try_crank(&mut rng).await == TryCrankOutcome::NoEventsToProcess {
                retry_count -= 1;
                if retry_count == 0 {
                    finished = true;
                    break;
                }
                time::sleep(POLL_INTERVAL).await;
            }
        }

        // Expect that the block was marked complete by the event generated by the accumulator.
        let expected_block = runner
            .reactor()
            .storage
            .read_highest_complete_block()
            .unwrap()
            .unwrap();
        assert_eq!(expected_block.height(), block_1.height());
    }
}
