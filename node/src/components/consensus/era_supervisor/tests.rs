use anyhow::Error;
use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use derive_more::From;
use futures::channel::oneshot;
use prometheus::Registry;

use casper_types::auction::ValidatorWeights;

use super::*;
use crate::{
    components::{
        consensus::{
            consensus_protocol::EraEnd,
            tests::mock_proto::{self, MockProto, NodeId},
            Config,
        },
        Component,
    },
    crypto::{
        asymmetric_key::{PublicKey, SecretKey},
        hash::Digest,
    },
    effect::{
        announcements::ConsensusAnnouncement,
        requests::{
            BlockExecutorRequest, BlockProposerRequest, BlockValidationRequest, ConsensusRequest,
            ContractRuntimeRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, Responder,
    },
    protocol,
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    testing::TestRng,
    types::{Block, BlockHash},
    utils::{self, External, Loadable},
    NodeRng,
};

type ClMessage = mock_proto::Message<ClContext>;

#[derive(Debug, From)]
enum Event {
    #[from]
    Consensus(super::Event<NodeId>),
    #[from]
    Network(NetworkRequest<NodeId, protocol::Message>),
    #[from]
    BlockProposer(BlockProposerRequest),
    #[from]
    ConsensusAnnouncement(ConsensusAnnouncement),
    #[from]
    BlockExecutor(BlockExecutorRequest),
    #[from]
    BlockValidator(BlockValidationRequest<ProtoBlock, NodeId>),
    #[from]
    Storage(StorageRequest),
    #[from]
    ContractRuntime(ContractRuntimeRequest),
}

struct MockReactor {
    scheduler: &'static Scheduler<Event>,
}

impl MockReactor {
    fn new() -> Self {
        MockReactor {
            scheduler: utils::leak(Scheduler::<Event>::new(QueueKind::weights())),
        }
    }

    /// Checks and responds to a block validation request.
    async fn expect_block_validation(
        &self,
        expected_block: &ProtoBlock,
        expected_sender: NodeId,
        expected_timestamp: Timestamp,
        valid: bool,
    ) {
        let (event, _) = self.scheduler.pop().await;
        if let Event::BlockValidator(BlockValidationRequest {
            block,
            sender,
            responder,
            block_timestamp,
        }) = event
        {
            assert_eq!(expected_block, &block);
            assert_eq!(expected_sender, sender);
            assert_eq!(expected_timestamp, block_timestamp);
            responder.respond((valid, block)).await;
        } else {
            panic!(
                "unexpected event: {:?}, expected block validation request",
                event
            );
        }
    }

    async fn expect_send_message(&self, peer: NodeId) {
        let (event, _) = self.scheduler.pop().await;
        if let Event::Network(NetworkRequest::SendMessage {
            dest, responder, ..
        }) = event
        {
            assert_eq!(peer, dest);
            responder.respond(()).await;
        } else {
            panic!(
                "unexpected event: {:?}, expected send message request",
                event
            );
        }
    }

    async fn expect_broadcast(&self) {
        let (event, _) = self.scheduler.pop().await;
        if let Event::Network(NetworkRequest::Broadcast { responder, .. }) = event {
            responder.respond(()).await;
        } else {
            panic!("unexpected event: {:?}, expected broadcast request", event);
        }
    }

    async fn expect_proposed(&self) -> ProtoBlock {
        let (event, _) = self.scheduler.pop().await;
        if let Event::ConsensusAnnouncement(ConsensusAnnouncement::Proposed(proto_block)) = event {
            proto_block
        } else {
            panic!(
                "unexpected event: {:?}, expected announcement of proposed proto block",
                event
            );
        }
    }

    async fn expect_finalized(&self) -> FinalizedBlock {
        let (event, _) = self.scheduler.pop().await;
        if let Event::ConsensusAnnouncement(ConsensusAnnouncement::Finalized(fb)) = event {
            *fb
        } else {
            panic!(
                "unexpected event: {:?}, expected announcement of finalized block",
                event
            );
        }
    }

    async fn expect_execute(&self) -> FinalizedBlock {
        let (event, _) = self.scheduler.pop().await;
        if let Event::BlockExecutor(BlockExecutorRequest::ExecuteBlock(fb)) = event {
            fb
        } else {
            panic!(
                "unexpected event: {:?}, expected block execution request",
                event
            );
        }
    }

    async fn expect_announce_block_handled(&self) -> Box<BlockHeader> {
        let (event, _) = self.scheduler.pop().await;
        if let Event::ConsensusAnnouncement(ConsensusAnnouncement::Handled(block_header)) = event {
            block_header
        } else {
            panic!(
                "unexpected event: {:?}, expected block handled announcement",
                event
            );
        }
    }

    async fn expect_validator_weights(&self, weights: ValidatorWeights) {
        let (event, _) = self.scheduler.pop().await;
        if let Event::ContractRuntime(ContractRuntimeRequest::GetValidatorWeightsByEraId {
            responder,
            ..
        }) = event
        {
            responder.respond(Ok(Some(weights))).await;
        } else {
            panic!(
                "unexpected event: {:?}, expected validator weights request",
                event
            );
        }
    }

    async fn expect_get_block_at_height(&self, block: Block) {
        let (event, _) = self.scheduler.pop().await;
        if let Event::Storage(StorageRequest::GetBlockAtHeight { responder, .. }) = event {
            // TODO: Assert expected height.
            responder.respond(Some(block)).await;
        } else {
            panic!(
                "unexpected event: {:?}, expected block at height request",
                event
            );
        }
    }
}

/// Loads the local chainspec and overrides timestamp and genesis account with the given stakes.
fn new_test_chainspec(stakes: Vec<(PublicKey, u64)>) -> Chainspec {
    let mut chainspec = Chainspec::from_resources("local/chainspec.toml");
    chainspec.genesis.accounts = stakes
        .into_iter()
        .map(|(pk, stake)| {
            let motes = Motes::new(stake.into());
            GenesisAccount::new(pk.into(), pk.to_account_hash(), motes, motes)
        })
        .collect();
    chainspec.genesis.timestamp = Timestamp::now();
    chainspec.genesis.highway_config.genesis_era_start_timestamp = chainspec.genesis.timestamp;

    // Every era has exactly three blocks.
    chainspec.genesis.highway_config.minimum_era_height = 2;
    chainspec.genesis.highway_config.era_duration = 0.into();
    chainspec
}

async fn propose_and_finalize(
    es: &mut EraSupervisor<NodeId>,
    proposer: PublicKey,
    accusations: Vec<PublicKey>,
    parent: BlockHash,
    rng: &mut NodeRng,
) -> Block {
    let reactor = MockReactor::new();
    let effect_builder = EffectBuilder::new(EventQueueHandle::new(reactor.scheduler));
    let era_id = es.current_era;
    let mut handle = |es: &mut EraSupervisor<NodeId>, event: super::Event<NodeId>| {
        es.handle_event(effect_builder, rng, event)
    };

    // Propose a new block. We receive that proposal via node 1.
    let proto_block = ProtoBlock::new(vec![], true);
    let candidate_block = CandidateBlock::new(proto_block.clone(), accusations.clone());
    let timestamp = Timestamp::now();
    let event = ClMessage::BlockByOtherValidator {
        value: candidate_block.clone(),
        timestamp,
        proposer,
    }
    .received(NodeId(1), era_id);
    let mut effects = handle(es, event);
    let validate = tokio::spawn(effects.pop().unwrap());
    reactor
        .expect_block_validation(&proto_block, NodeId(1), timestamp, true)
        .await;
    let rv_event = validate.await.unwrap().pop().unwrap();

    // As a result, the era supervisor should request validation of the proto block and evidence
    // against all accused validators.
    for _ in &accusations {
        let request_evidence = tokio::spawn(effects.pop().unwrap());
        reactor.expect_send_message(NodeId(1)).await; // Sending request for evidence.
        request_evidence.await.unwrap();
    }
    assert!(effects.is_empty());

    // Node 1 replies with requested evidence.
    for pk in &accusations {
        let event = ClMessage::Evidence(*pk).received(NodeId(1), era_id);
        let mut effects = handle(es, event);
        let broadcast_evidence = tokio::spawn(effects.pop().unwrap());
        assert!(effects.is_empty());
        reactor.expect_broadcast().await; //Gossip evidence to other nodes.
        broadcast_evidence.await.unwrap();
    }

    // The block validator returns true: The deploys in the block are valid. That completes our
    // requirements for the proposed block. The era supervisor announces that it has passed the
    // proposed block to the consensus protocol.
    let mut effects = handle(es, rv_event);
    let announce_proposed = tokio::spawn(effects.pop().unwrap());
    assert!(effects.is_empty());
    assert_eq!(proto_block, reactor.expect_proposed().await);
    announce_proposed.await.unwrap();

    // Node 1 now sends us another message that is sufficient for the protocol to finalize the
    // block. The era supervisor is expected to announce finalization, and to request execution.
    let event = ClMessage::FinalizeBlock.received(NodeId(1), era_id);
    let mut effects = handle(es, event);
    tokio::spawn(effects.pop().unwrap()).await.unwrap();
    tokio::spawn(effects.pop().unwrap()).await.unwrap();
    assert!(effects.is_empty());

    let fb = reactor.expect_execute().await;
    assert_eq!(fb, reactor.expect_finalized().await);

    let block = Block::new(parent, Default::default(), Default::default(), fb.clone());
    let block_header = Box::new(block.header().clone());
    let (sender, receiver) = oneshot::channel();
    let responder = Responder::create(sender);
    let receiver = tokio::spawn(receiver);
    let request = ConsensusRequest::HandleLinearBlock(block_header.clone(), responder);
    let mut effects = handle(es, request.into());
    if block_header.switch_block() {
        let create_new_era = tokio::spawn(effects.pop().unwrap());
        let finality_sig = tokio::spawn(effects.pop().unwrap()); // Finality signature.
        assert!(effects.is_empty());
        let weights = vec![(block_header.proposer().clone().into(), 10.into())]
            .into_iter()
            .collect();
        reactor.expect_validator_weights(weights).await;
        // TODO: Return the correct block!
        reactor.expect_get_block_at_height(block.clone()).await;
        reactor.expect_get_block_at_height(block.clone()).await;
        finality_sig.await.unwrap();
        let mut events = create_new_era.await.unwrap();
        let create_new_era_event = events.pop().unwrap();
        assert!(events.is_empty());

        let mut effects = handle(es, create_new_era_event);
        let block_h = tokio::spawn(effects.pop().unwrap()); // Announce block handled.
        assert!(effects.is_empty());
        assert_eq!(block_header, reactor.expect_announce_block_handled().await);
        block_h.await.unwrap();
    } else {
        let block_h = tokio::spawn(effects.pop().unwrap()); // Announce block handled.
        tokio::spawn(effects.pop().unwrap()).await.unwrap(); // Finality signature.
        assert!(effects.is_empty());
        assert_eq!(block_header, reactor.expect_announce_block_handled().await);
        block_h.await.unwrap();
    };
    let sig = receiver.await.unwrap().unwrap();
    asymmetric_key::verify(block.hash(), &sig, &es.public_signing_key).unwrap();

    block
}

#[tokio::test]
async fn finalize_blocks_and_switch_eras() -> Result<(), Error> {
    let mut rng = TestRng::new();

    let alice_sk = SecretKey::random(&mut rng);
    let alice_pk = PublicKey::from(&alice_sk);
    let bob_sk = SecretKey::random(&mut rng);
    let bob_pk = PublicKey::from(&bob_sk);

    let (mut es, effects) = {
        let chainspec = new_test_chainspec(vec![(alice_pk, 10), (bob_pk, 100)]);
        let config = Config {
            secret_key_path: External::Loaded(alice_sk),
        };

        let registry = Registry::new();
        let reactor = MockReactor::new();
        let effect_builder = EffectBuilder::new(EventQueueHandle::new(reactor.scheduler));

        // Initialize the era supervisor. There are two validators, Alice and Bob. This instance,
        // however, is only a passive observer.
        EraSupervisor::new(
            Timestamp::now(),
            WithDir::new("tmp", config),
            effect_builder,
            vec![
                (alice_pk, Motes::new(10.into())),
                (bob_pk, Motes::new(100.into())),
            ],
            &chainspec,
            Default::default(), // genesis state root hash
            &registry,
            Box::new(MockProto::new_boxed),
            &mut rng,
        )?
    };
    assert!(effects.is_empty());

    let parent_hash = BlockHash::from(Digest::from([0; Digest::LENGTH]));
    let block = propose_and_finalize(&mut es, bob_pk, vec![alice_pk], parent_hash, &mut rng).await;
    let fb: FinalizedBlock = block.header().clone().into();
    let expected_fb = FinalizedBlock::new(
        fb.proto_block().clone(),
        fb.timestamp(),
        None, // not the era's last block
        EraId(0),
        0, // height
        bob_pk,
    );
    assert_eq!(expected_fb, fb);
    let parent_hash = *block.hash();

    let block = propose_and_finalize(&mut es, bob_pk, vec![], parent_hash, &mut rng).await;
    let fb: FinalizedBlock = block.header().clone().into();
    let expected_fb = FinalizedBlock::new(
        fb.proto_block().clone(),
        fb.timestamp(),
        Some(EraEnd {
            equivocators: vec![alice_pk],
            rewards: Default::default(),
        }), // the era's last block
        EraId(0),
        1, // height
        bob_pk,
    );
    assert_eq!(expected_fb, fb);
    let parent_hash = *block.hash();

    let block = propose_and_finalize(&mut es, bob_pk, vec![], parent_hash, &mut rng).await;
    let fb: FinalizedBlock = block.header().clone().into();
    let expected_fb = FinalizedBlock::new(
        fb.proto_block().clone(),
        fb.timestamp(),
        None, // not the era's last block
        EraId(1),
        2, // height
        bob_pk,
    );
    assert_eq!(expected_fb, fb);

    Ok(())
}
