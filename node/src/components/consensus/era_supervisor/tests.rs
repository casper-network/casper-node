use anyhow::Error;
use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use derive_more::From;
use prometheus::Registry;

use super::*;
use crate::{
    components::{
        consensus::{
            tests::mock_proto::{self, MockProto, NodeId},
            Config,
        },
        Component,
    },
    crypto::asymmetric_key::{PublicKey, SecretKey},
    effect::{
        announcements::ConsensusAnnouncement,
        requests::{
            BlockExecutorRequest, BlockProposerRequest, BlockValidationRequest,
            ContractRuntimeRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder,
    },
    protocol,
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    testing::TestRng,
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
    chainspec.genesis.timestamp = Timestamp::now() + 1000.into();
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
    rng: &mut NodeRng,
) -> FinalizedBlock {
    let reactor = MockReactor::new();
    let effect_builder = EffectBuilder::new(EventQueueHandle::new(reactor.scheduler));
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
    .received(NodeId(1), EraId(0)); // TODO: Compute era ID.
    let mut effects = handle(es, event);

    // As a result, the era supervisor should request validation of the proto block and evidence
    // against Alice.
    let validate = tokio::spawn(effects.pop().unwrap());
    let request_evidence = tokio::spawn(effects.pop().unwrap());
    assert!(effects.is_empty());
    // TODO: Requests depend on accusations.
    reactor.expect_send_message(NodeId(1)).await; // Sending request for evidence.
    request_evidence.await.unwrap();

    // The block validator returns true: The deploys in the block are valid. As a result, we don't
    // do anything yet (no new effects): We have to wait for the evidence first.
    reactor
        .expect_block_validation(&proto_block, NodeId(1), timestamp, true)
        .await;
    let rv_event = validate.await.unwrap().pop().unwrap();
    let effects = handle(es, rv_event);
    assert!(effects.is_empty());

    // Node 1 replies with evidence of Alice's fault. That completes our requirements for the
    // proposed block. The era supervisor announces that it has passed the proposed block to the
    // consensus protocol. It also gossips the new evidence to other nodes.
    // TODO: Only pass in the requested evidence.
    let event = ClMessage::Evidence(accusations[0].clone()).received(NodeId(1), EraId(0));
    let mut effects = handle(es, event);
    let announce_proposed = tokio::spawn(effects.pop().unwrap());
    let broadcast_evidence = tokio::spawn(effects.pop().unwrap());
    assert!(effects.is_empty());
    assert_eq!(proto_block, reactor.expect_proposed().await);
    reactor.expect_broadcast().await; //Gossip evidence to other nodes.
    announce_proposed.await.unwrap();
    broadcast_evidence.await.unwrap();

    // Node 1 now sends us another message that is sufficient for the protocol to finalize the
    // block. The era supervisor is expected to announce finalization, and to request execution.
    let event = ClMessage::FinalizeBlock.received(NodeId(1), EraId(0));
    let mut effects = handle(es, event);
    tokio::spawn(effects.pop().unwrap()).await.unwrap();
    tokio::spawn(effects.pop().unwrap()).await.unwrap();
    assert!(effects.is_empty());

    let fb = reactor.expect_execute().await;
    assert_eq!(fb, reactor.expect_finalized().await);
    fb
}

#[tokio::test]
async fn cross_era_slashing() -> Result<(), Error> {
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
    let fb = propose_and_finalize(&mut es, bob_pk, vec![alice_pk], &mut rng).await;
    let expected_fb = FinalizedBlock::new(
        fb.proto_block().clone(),
        fb.timestamp(),
        None, // not the era's last block
        EraId(0),
        0, // height
        bob_pk,
    );
    assert_eq!(expected_fb, fb);

    Ok(())
}
