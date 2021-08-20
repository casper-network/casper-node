#![cfg(test)]

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
const TIMEOUT: Duration = Duration::from_secs(10);

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[allow(clippy::large_enum_variant)]
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
    FromPeerRegression,
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
            | TestScenario::FromPeerRepeatedValidDeploy
            | TestScenario::FromPeerRegression => Source::Peer(NodeId::random(rng)),
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
            | TestScenario::FromClientRepeatedValidDeploy
            | TestScenario::FromPeerRegression => (),
        }
        deploy
    }

    fn is_from_peer(&self) -> bool {
        match self {
            TestScenario::FromPeerInvalidDeploy
            | TestScenario::FromPeerValidDeploy
            | TestScenario::FromPeerRepeatedValidDeploy
            | TestScenario::FromPeerRegression => true,
            TestScenario::FromClientInvalidDeploy
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientValidDeploy
            | TestScenario::FromClientRepeatedValidDeploy => false,
        }
    }

    fn is_valid_deploy_case(&self) -> bool {
        match self {
            TestScenario::FromPeerRepeatedValidDeploy
            | TestScenario::FromPeerValidDeploy
            | TestScenario::FromClientRepeatedValidDeploy
            | TestScenario::FromClientValidDeploy => true,
            TestScenario::FromPeerInvalidDeploy
            | TestScenario::FromPeerRegression
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInvalidDeploy => false,
        }
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
            Event::ContractRuntime(event) => {
                // Contract runtime requests should not be made in the case of a deploy sent
                // by a peer.
                assert!(!self.test_scenario.is_from_peer());
                match event {
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
                        let motes =
                            if self.test_scenario == TestScenario::FromClientInsufficientBalance {
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
                }
            }
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

fn put_deploy_to_storage(
    deploy: Box<Deploy>,
    responder: Responder<bool>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .into_inner()
            .schedule(
                StorageRequest::PutDeploy { deploy, responder },
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

async fn run_deploy_acceptor_without_timeout(
    test_scenario: TestScenario,
) -> Result<(), super::Error> {
    let mut rng = crate::new_rng();

    let mut runner: Runner<ConditionCheckReactor<Reactor>> =
        Runner::new(test_scenario, &mut rng).await.unwrap();

    let block = Box::new(Block::random(&mut rng));
    // Create a responder to assert that the block was successfully injected into storage.
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

    // Create a responder to assert the validity of the deploy
    let (deploy_sender, deploy_receiver) = oneshot::channel();
    let deploy_responder = Responder::create(deploy_sender);

    // Create a deploy specific to the test scenario
    let deploy = test_scenario.deploy(&mut rng);
    // Mark the source as either a peer or a client depending on the scenario.
    let source = test_scenario.source(&mut rng);

    {
        // Inject the deploy artificially into storage to simulate a previously seen deploy.
        if test_scenario == TestScenario::FromClientRepeatedValidDeploy
            || test_scenario == TestScenario::FromPeerRepeatedValidDeploy
        {
            let injected_deploy = Box::new(deploy.clone());
            let (injected_sender, injected_receiver) = oneshot::channel();
            let injected_responder = Responder::create(injected_sender);
            runner
                .process_injected_effects(put_deploy_to_storage(
                    injected_deploy,
                    injected_responder,
                ))
                .await;
            while runner.try_crank(&mut rng).await.is_none() {
                time::sleep(POLL_INTERVAL).await;
            }
            // Check that the "previously seen" deploy is present in storage.
            assert!(injected_receiver.await.unwrap());
        }
    }

    runner
        .process_injected_effects(schedule_accept_deploy(
            Box::new(deploy.clone()),
            source,
            deploy_responder,
        ))
        .await;

    // Tests where the deploy is already in storage will not trigger any deploy acceptor
    // announcement, so use the deploy acceptor `PutToStorage` event as the condition.
    let stopping_condition = move |event: &Event| -> bool {
        match test_scenario {
            // Check that invalid deploys sent by a client raise the `InvalidDeploy` announcement
            // with the appropriate source.
            TestScenario::FromClientInvalidDeploy
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInsufficientBalance => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                        source: Source::Client,
                        ..
                    })
                )
            }
            // Check that invalid deploys sent by a peer raise the `InvalidDeploy` announcement
            // with the appropriate source.
            TestScenario::FromPeerInvalidDeploy => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                        source: Source::Peer(_),
                        ..
                    })
                )
            }
            // Check that a, new and valid, deploy sent by a peer raises an `AcceptedNewDeploy`
            // announcement with the appropriate source.
            TestScenario::FromPeerValidDeploy => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy {
                            source: Source::Peer(_),
                            ..
                        }
                    )
                )
            }
            // Check that a, new and valid, deploy sent by a client raises an `AcceptedNewDeploy`
            // announcement with the appropriate source.
            TestScenario::FromClientValidDeploy => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy {
                            source: Source::Client,
                            ..
                        }
                    )
                )
            }
            // Check that repeated valid deploys raise the `PutToStorageResult` with the
            // `is_new` flag as false.
            TestScenario::FromClientRepeatedValidDeploy
            | TestScenario::FromPeerRepeatedValidDeploy => {
                matches!(
                    event,
                    Event::DeployAcceptor(super::Event::PutToStorageResult { is_new: false, .. })
                )
            }
            // Deploys received from a peer should not raise `ContractRuntimeRequest` events
            // as part of the validation process.
            // This test scenario should therefore, keep running and eventually timeout.
            TestScenario::FromPeerRegression => {
                matches!(
                    event,
                    Event::ContractRuntime(ContractRuntimeRequest::Query { .. })
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

    {
        // Assert that the deploy is present in the case of a valid deploy.
        // Conversely, assert its absence in the invalid case.
        let is_in_storage = runner
            .reactor()
            .inner()
            .storage
            .get_deploy_by_hash(*deploy.id())
            .is_some();

        if test_scenario.is_valid_deploy_case() {
            assert!(is_in_storage)
        } else {
            assert!(!is_in_storage)
        }
    }

    deploy_receiver.await.unwrap()
}

async fn run_deploy_acceptor(test_scenario: TestScenario) -> Result<(), super::Error> {
    time::timeout(TIMEOUT, run_deploy_acceptor_without_timeout(test_scenario))
        .await
        .unwrap()
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer() {
    let result = run_deploy_acceptor(TestScenario::FromPeerValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_invalid_deploy_from_peer() {
    let result = run_deploy_acceptor(TestScenario::FromPeerInvalidDeploy).await;
    assert!(matches!(result, Err(super::Error::InvalidDeploy(_))))
}

#[tokio::test]
async fn should_accept_valid_deploy_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_invalid_deploy_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientInvalidDeploy).await;
    assert!(matches!(result, Err(super::Error::InvalidDeploy(_))))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_invalid_account() {
    let result = run_deploy_acceptor(TestScenario::FromClientMissingAccount).await;
    assert!(matches!(result, Err(super::Error::InvalidAccount)))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_insufficient_balance() {
    let result = run_deploy_acceptor(TestScenario::FromClientInsufficientBalance).await;
    assert!(matches!(result, Err(super::Error::InsufficientBalance)))
}

#[tokio::test]
async fn should_accept_repeated_valid_deploy_from_peer() {
    let result = run_deploy_acceptor(TestScenario::FromPeerRepeatedValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_repeated_valid_deploy_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientRepeatedValidDeploy).await;
    assert!(result.is_ok())
}

// The test scenario should timeout as the stopping condition of raising a `ContractRuntimeRequest`
// is never raised.
#[tokio::test]
async fn should_timeout_deploy_acceptor() {
    let result = time::timeout(
        TIMEOUT,
        run_deploy_acceptor_without_timeout(TestScenario::FromPeerRegression),
    )
    .await;
    assert!(result.is_err())
}
