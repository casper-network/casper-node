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
        storage::Storage,
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
    Storage(StorageRequest<Storage>),
    #[from]
    ContractRuntime(ContractRuntimeRequest),
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
    chainspec
}

/// Creates a new `EffectBuilder` with a new scheduler and queue.
fn new_effect_builder() -> EffectBuilder<Event> {
    let scheduler = utils::leak(Scheduler::<Event>::new(QueueKind::weights()));
    EffectBuilder::new(EventQueueHandle::new(&scheduler))
}

/// Test proposing and finalizing blocks.
// TODO: This is still incomplete, and basically just a stub to test using MockProto.
#[test]
fn era_supervisor() -> Result<(), Error> {
    let mut rng = TestRng::new();

    let alice_sk = SecretKey::random(&mut rng);
    let alice_pk = PublicKey::from(&alice_sk);
    let bob_sk = SecretKey::random(&mut rng);
    let bob_pk = PublicKey::from(&bob_sk);

    let chainspec = new_test_chainspec(vec![(alice_pk, 100)]);
    let config = Config {
        secret_key_path: External::Loaded(alice_sk),
    };

    let registry = Registry::new();
    let effect_builder = new_effect_builder();

    let (mut es, effects) = EraSupervisor::new(
        Timestamp::now(),
        WithDir::new("tmp", config),
        effect_builder,
        vec![(alice_pk, Motes::new(100.into()))],
        &chainspec,
        Default::default(), // genesis state root hash
        &registry,
        Box::new(MockProto::new_boxed),
        &mut rng,
    )?;
    assert!(effects.is_empty());

    let mut handle = |es: &mut EraSupervisor<NodeId>, event: super::Event<NodeId>| {
        es.handle_event(effect_builder, &mut rng, event)
    };

    let event = ClMessage::BlockByOtherValidator {
        value: CandidateBlock::new(ProtoBlock::new(vec![], true), vec![alice_pk]),
        timestamp: Timestamp::now(),
        proposer: bob_pk,
    }
    .received(NodeId(1), EraId(0));
    let effects = handle(&mut es, event);
    assert_eq!(effects.len(), 2); // Validate consensus value, request evidence against Alice.
    let event = ClMessage::FinalizeBlock.received(NodeId(1), EraId(0));
    let effects = handle(&mut es, event);
    assert_eq!(effects.len(), 2); // Announce and execute block.
    assert_eq!(&[alice_pk], &*es.active_eras[&EraId(0)].accusations());

    // TODO: Add a reactor to verify the effects.

    assert_eq!(EraId(0), es.current_era);
    Ok(())
}
