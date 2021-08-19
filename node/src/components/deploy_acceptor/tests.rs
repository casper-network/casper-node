#![cfg(test)]

// TODO -
//  * for each success case, check deploy is put to storage
//  * for all success cases, where the deploy is newly put to storage, check the deploy acceptor
//    announces the new deploy
//  * for all success cases, where the deploy is not put to storage (i.e. the deploy has already
//    been accepted), check the deploy acceptor doesn't make an announcement
//  * for all failure cases, ensure the deploy acceptor makes an announcement

use std::{
    collections::{BTreeMap, VecDeque},
    fmt::{self, Debug, Display, Formatter},
    time::Duration,
};

use derive_more::From;
use futures::channel::oneshot;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;
use tokio::time;

use casper_execution_engine::{
    core::engine_state::{BalanceResult, QueryResult, MAX_PAYMENT_AMOUNT},
    shared::{account::Account, stored_value::StoredValue},
    storage::trie::merkle_proof::TrieMerkleProof,
};
use casper_types::{CLValue, ProtocolVersion, URef, U512};

use super::*;
use crate::{
    components::storage::{self, Storage},
    effect::{
        announcements::{ControlAnnouncement, DeployAcceptorAnnouncement},
        requests::ContractRuntimeRequest,
        Responder,
    },
    reactor::{self, EventQueueHandle, QueueKind, Runner},
    testing::ConditionCheckReactor,
    types::{Block, Chainspec, Deploy, NodeId},
    utils::{Loadable, WithDir},
    NodeRng,
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
enum Event {
    #[from]
    Storage(#[serde(skip_serializing)] storage::Event),
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] super::Event),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement<NodeId>),
    #[from]
    ContractRuntime(#[serde(skip_serializing)] ContractRuntimeRequest),
}

impl ReactorEvent for Event {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

impl From<StorageRequest> for Event {
    fn from(request: StorageRequest) -> Self {
        Event::Storage(storage::Event::from(request))
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::DeployAcceptor(event) => write!(formatter, "deploy acceptor: {}", event),
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(formatter, "deploy-acceptor announcement: {}", ann)
            }

            Event::ContractRuntime(event) => {
                write!(formatter, "contract-runtime event: {:?}", event)
            }
        }
    }
}

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
enum Error {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TestScenario {
    FromPeerInvalidDeploy,
    FromPeerValidDeploy,
    FromPeerRepeatedValidDeploy,
    FromClientInvalidDeploy,
    FromClientMissingAccount,
    FromClientInsufficientBalance,
    FromClientValidDeploy,
    FromClientRepeatedValidDeploy,
}

impl TestScenario {
    fn source(&self, rng: &mut NodeRng) -> Source<NodeId> {
        match self {
            TestScenario::FromPeerInvalidDeploy
            | TestScenario::FromPeerValidDeploy
            | TestScenario::FromPeerRepeatedValidDeploy => Source::Peer(NodeId::random(rng)),
            TestScenario::FromClientInvalidDeploy
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientValidDeploy
            | TestScenario::FromClientRepeatedValidDeploy => Source::Client,
        }
    }

    fn deploy(&self, rng: &mut NodeRng) -> Deploy {
        let mut deploy = Deploy::random(rng);
        match self {
            TestScenario::FromPeerInvalidDeploy | TestScenario::FromClientInvalidDeploy => {
                deploy.invalidate()
            }
            TestScenario::FromPeerValidDeploy
            | TestScenario::FromPeerRepeatedValidDeploy
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientValidDeploy
            | TestScenario::FromClientRepeatedValidDeploy => (),
        }
        deploy
    }
}

struct Reactor {
    storage: Storage,
    deploy_acceptor: DeployAcceptor,
    _storage_tempdir: TempDir,
    test_scenario: TestScenario,
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = TestScenario;
    type Error = Error;

    fn new(
        config: Self::Config,
        _registry: &Registry,
        _event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (storage_config, storage_tempdir) = storage::Config::default_for_tests();
        let storage_withdir = WithDir::new(storage_tempdir.path(), storage_config);
        let storage = Storage::new(
            &storage_withdir,
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            false,
            "test",
        )
        .unwrap();

        let deploy_acceptor = DeployAcceptor::new(
            super::Config::new(true),
            &Chainspec::from_resources("local"),
        );

        let reactor = Reactor {
            storage,
            deploy_acceptor,
            _storage_tempdir: storage_tempdir,
            test_scenario: config,
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
            Event::DeployAcceptor(event) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::ControlAnnouncement(ctrl_ann) => {
                panic!("unhandled control announcement: {}", ctrl_ann)
            }
            Event::DeployAcceptorAnnouncement(_) => {
                // We do not care about deploy acceptor announcements in the acceptor tests.
                Effects::new()
            }
            Event::ContractRuntime(event) => match event {
                ContractRuntimeRequest::Query {
                    query_request,
                    responder,
                } => {
                    let query_result =
                        if self.test_scenario == TestScenario::FromClientMissingAccount {
                            QueryResult::ValueNotFound(String::new())
                        } else if let Key::Account(account_hash) = query_request.key() {
                            let preset_account =
                                Account::create(account_hash, BTreeMap::new(), URef::default());
                            QueryResult::Success {
                                value: Box::new(StoredValue::Account(preset_account)),
                                proofs: vec![],
                            }
                        } else {
                            panic!("expect only queries using Key::Account variant");
                        };
                    responder.respond(Ok(query_result)).ignore()
                }
                ContractRuntimeRequest::GetBalance {
                    balance_request,
                    responder,
                } => {
                    let proof = TrieMerkleProof::new(
                        balance_request.purse_uref().into(),
                        StoredValue::CLValue(CLValue::from_t(()).expect("should get CLValue")),
                        VecDeque::new(),
                    );
                    let motes = if self.test_scenario == TestScenario::FromClientInsufficientBalance
                    {
                        MAX_PAYMENT_AMOUNT - 1
                    } else {
                        MAX_PAYMENT_AMOUNT
                    };
                    let balance_result = BalanceResult::Success {
                        motes: U512::from(motes),
                        proof: Box::new(proof),
                    };
                    responder.respond(Ok(balance_result)).ignore()
                }
                _ => panic!("should not receive {:?}", event),
            },
        }
    }

    fn maybe_exit(&self) -> Option<crate::reactor::ReactorExit> {
        panic!()
    }
}

fn put_block_to_storage(
    block: Box<Block>,
    responder: Responder<bool>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .into_inner()
            .schedule(
                StorageRequest::PutBlock { block, responder },
                QueueKind::Regular,
            )
            .ignore()
    }
}

fn schedule_accept_deploy(
    deploy: Box<Deploy>,
    source: Source<NodeId>,
    responder: Responder<Result<(), super::Error>>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .into_inner()
            .schedule(
                super::Event::Accept {
                    deploy,
                    source,
                    responder: Some(responder),
                },
                QueueKind::Regular,
            )
            .ignore()
    }
}

async fn run_deploy_acceptor(test_scenario: TestScenario) -> Result<(), super::Error> {
    let mut rng = crate::new_rng();

    let mut runner: Runner<ConditionCheckReactor<Reactor>> =
        Runner::new(test_scenario, &mut rng).await.unwrap();

    let block = Box::new(Block::random(&mut rng));
    let (block_sender, block_receiver) = oneshot::channel();
    let block_responder = Responder::create(block_sender);

    runner
        .process_injected_effects(put_block_to_storage(block, block_responder))
        .await;

    // There's only one scheduled event, so we only need to try cranking until the first time it
    // returns `Some`.
    while runner.try_crank(&mut rng).await.is_none() {
        time::sleep(POLL_INTERVAL).await;
    }
    assert!(block_receiver.await.unwrap());

    let (deploy_sender, deploy_receiver) = oneshot::channel();
    let deploy_responder = Responder::create(deploy_sender);

    let deploy = test_scenario.deploy(&mut rng);
    let source = test_scenario.source(&mut rng);

    runner
        .process_injected_effects(schedule_accept_deploy(
            Box::new(deploy),
            source,
            deploy_responder,
        ))
        .await;

    // Tests where the deploy is already in storage will not trigger any deploy acceptor
    // announcement, so use the deploy acceptor `PutToStorage` event as the condition.
    let stopping_condition = move |event: &Event| -> bool {
        match test_scenario {
            TestScenario::FromPeerInvalidDeploy
            | TestScenario::FromPeerValidDeploy
            | TestScenario::FromClientInvalidDeploy
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientValidDeploy => {
                matches!(event, Event::DeployAcceptorAnnouncement(_))
            }
            TestScenario::FromClientRepeatedValidDeploy
            | TestScenario::FromPeerRepeatedValidDeploy => {
                matches!(
                    event,
                    Event::DeployAcceptor(super::Event::PutToStorageResult { is_new: false, .. })
                )
            }
        }
    };
    runner
        .reactor_mut()
        .set_condition_checker(Box::new(stopping_condition));

    loop {
        if runner.try_crank(&mut rng).await.is_some() {
            if runner.reactor().condition_result() {
                break;
            }
        } else {
            time::sleep(POLL_INTERVAL).await;
        }
    }

    deploy_receiver.await.unwrap()
}

#[tokio::test]
async fn should_accept_from_peer() {
    let result = run_deploy_acceptor(TestScenario::FromPeerValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_invalid_from_peer() {
    let result = run_deploy_acceptor(TestScenario::FromPeerInvalidDeploy).await;
    assert!(matches!(result, Err(super::Error::InvalidDeploy(_))))
}

#[tokio::test]
async fn should_accept_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_invalid_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientInvalidDeploy).await;
    assert!(matches!(result, Err(super::Error::InvalidDeploy(_))))
}

#[tokio::test]
async fn should_reject_from_client_for_invalid_account() {
    let result = run_deploy_acceptor(TestScenario::FromClientMissingAccount).await;
    assert!(matches!(result, Err(super::Error::InvalidAccount)))
}

#[tokio::test]
async fn should_reject_from_client_for_insufficient_balance() {
    let result = run_deploy_acceptor(TestScenario::FromClientInsufficientBalance).await;
    assert!(matches!(result, Err(super::Error::InsufficientBalance)))
}
