// Unrestricted event size is okay in tests.
#![allow(clippy::large_enum_variant)]
#![cfg(test)]
use std::{
    collections::{BTreeSet, HashMap},
    iter,
    sync::Arc,
};

use derive_more::{Display, From};
use prometheus::Registry;
use rand::Rng;
use reactor::ReactorEvent;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;
use tokio::time;
use tracing::debug;

use casper_types::{testing::TestRng, ProtocolVersion, TimeDiff};

use super::*;
use crate::{
    components::{
        deploy_acceptor,
        in_memory_network::{self, InMemoryNetwork, NetworkController},
        network::{GossipedAddress, Identity as NetworkIdentity},
        storage::{self, Storage},
    },
    effect::{
        announcements::{
            ControlAnnouncement, DeployAcceptorAnnouncement, FatalAnnouncement,
            GossiperAnnouncement, RpcServerAnnouncement,
        },
        incoming::{
            ConsensusDemand, ConsensusMessageIncoming, FinalitySignatureIncoming,
            NetRequestIncoming, NetResponseIncoming, TrieDemand, TrieRequestIncoming,
            TrieResponseIncoming,
        },
        Responder,
    },
    protocol::Message as NodeMessage,
    reactor::{self, EventQueueHandle, QueueKind, Runner, TryCrankOutcome},
    testing::{
        self,
        network::{NetworkedReactor, TestingNetwork},
        ConditionCheckReactor, FakeDeployAcceptor,
    },
    types::{Block, Chainspec, ChainspecRawBytes, Deploy, FinalitySignature, NodeId},
    utils::WithDir,
    NodeRng,
};

const RECENT_ERA_COUNT: u64 = 5;
const MAX_TTL: TimeDiff = TimeDiff::from_seconds(86400);
const EXPECTED_GOSSIP_TARGET: GossipTarget = GossipTarget::All;

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize, Display)]
#[must_use]
enum Event {
    #[from]
    Network(in_memory_network::Event<NodeMessage>),
    #[from]
    Storage(storage::Event),
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] deploy_acceptor::Event),
    #[from]
    DeployGossiper(super::Event<Deploy>),
    #[from]
    NetworkRequest(NetworkRequest<NodeMessage>),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement),
    #[from]
    DeployGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<Deploy>),
    #[from]
    DeployGossiperIncoming(GossiperIncoming<Deploy>),
}

impl ReactorEvent for Event {
    fn is_control(&self) -> bool {
        false
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        None
    }
}

impl From<NetworkRequest<Message<Deploy>>> for Event {
    fn from(request: NetworkRequest<Message<Deploy>>) -> Self {
        Event::NetworkRequest(request.map_payload(NodeMessage::from))
    }
}

trait Unhandled {}

impl<T: Unhandled> From<T> for Event {
    fn from(_: T) -> Self {
        unimplemented!("not handled in gossiper tests")
    }
}

impl Unhandled for ConsensusDemand {}
impl Unhandled for ControlAnnouncement {}
impl Unhandled for FatalAnnouncement {}
impl Unhandled for ConsensusMessageIncoming {}
impl Unhandled for GossiperIncoming<Block> {}
impl Unhandled for GossiperIncoming<FinalitySignature> {}
impl Unhandled for GossiperIncoming<GossipedAddress> {}
impl Unhandled for NetRequestIncoming {}
impl Unhandled for NetResponseIncoming {}
impl Unhandled for TrieRequestIncoming {}
impl Unhandled for TrieDemand {}
impl Unhandled for TrieResponseIncoming {}
impl Unhandled for FinalitySignatureIncoming {}

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
enum Error {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

struct Reactor {
    network: InMemoryNetwork<NodeMessage>,
    storage: Storage,
    fake_deploy_acceptor: FakeDeployAcceptor,
    deploy_gossiper: Gossiper<{ Deploy::ID_IS_COMPLETE_ITEM }, Deploy>,
    _storage_tempdir: TempDir,
}

impl Drop for Reactor {
    fn drop(&mut self) {
        NetworkController::<NodeMessage>::remove_node(&self.network.node_id())
    }
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = Config;
    type Error = Error;

    fn new(
        config: Self::Config,
        _chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let network = NetworkController::create_node(event_queue, rng);

        let (storage_config, storage_tempdir) = storage::Config::default_for_tests();
        let storage_withdir = WithDir::new(storage_tempdir.path(), storage_config);
        let storage = Storage::new(
            &storage_withdir,
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            "test",
            MAX_TTL,
            RECENT_ERA_COUNT,
            Some(registry),
            false,
        )
        .unwrap();

        let fake_deploy_acceptor = FakeDeployAcceptor::new();
        let deploy_gossiper = Gossiper::<{ Deploy::ID_IS_COMPLETE_ITEM }, _>::new(
            "deploy_gossiper",
            config,
            registry,
        )?;

        let reactor = Reactor {
            network,
            storage,
            fake_deploy_acceptor,
            deploy_gossiper,
            _storage_tempdir: storage_tempdir,
        };

        Ok((reactor, Effects::new()))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        trace!(?event);
        match event {
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::DeployAcceptor(event) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.fake_deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::DeployGossiper(super::Event::ItemReceived {
                item_id,
                source,
                target,
            }) => {
                // Ensure the correct target type for deploys is provided.
                assert_eq!(target, EXPECTED_GOSSIP_TARGET);
                let event = super::Event::ItemReceived {
                    item_id,
                    source,
                    target,
                };
                reactor::wrap_effects(
                    Event::DeployGossiper,
                    self.deploy_gossiper
                        .handle_event(effect_builder, rng, event),
                )
            }
            Event::DeployGossiper(event) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequest(NetworkRequest::Gossip {
                payload,
                gossip_target,
                count,
                exclude,
                auto_closing_responder,
            }) => {
                // Ensure the correct target type for deploys is carried through to the `Network`.
                assert_eq!(gossip_target, EXPECTED_GOSSIP_TARGET);
                let request = NetworkRequest::Gossip {
                    payload,
                    gossip_target,
                    count,
                    exclude,
                    auto_closing_responder,
                };
                reactor::wrap_effects(
                    Event::Network,
                    self.network
                        .handle_event(effect_builder, rng, request.into()),
                )
            }
            Event::NetworkRequest(request) => reactor::wrap_effects(
                Event::Network,
                self.network
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::StorageRequest(request) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived {
                deploy,
                responder,
            }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::Client,
                    maybe_responder: responder,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployAcceptor(event))
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::AcceptedNewDeploy {
                deploy,
                source,
            }) => {
                let event = super::Event::ItemReceived {
                    item_id: deploy.gossip_id(),
                    source,
                    target: deploy.gossip_target(),
                };
                self.dispatch_event(effect_builder, rng, Event::DeployGossiper(event))
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                deploy: _,
                source: _,
            }) => Effects::new(),
            Event::DeployGossiperAnnouncement(GossiperAnnouncement::NewItemBody {
                item,
                sender,
            }) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.fake_deploy_acceptor.handle_event(
                    effect_builder,
                    rng,
                    deploy_acceptor::Event::Accept {
                        deploy: item,
                        source: Source::Peer(sender),
                        maybe_responder: None,
                    },
                ),
            ),
            Event::DeployGossiperAnnouncement(_ann) => Effects::new(),
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network.handle_event(effect_builder, rng, event),
            ),
            Event::DeployGossiperIncoming(incoming) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
        }
    }
}

impl NetworkedReactor for Reactor {
    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn announce_deploy_received(
    deploy: Box<Deploy>,
    responder: Option<Responder<Result<(), deploy_acceptor::Error>>>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .announce_deploy_received(deploy, responder)
            .ignore()
    }
}

async fn run_gossip(rng: &mut TestRng, network_size: usize, deploy_count: usize) {
    const TIMEOUT: Duration = Duration::from_secs(20);
    const QUIET_FOR: Duration = Duration::from_millis(50);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();

    // Add `network_size` nodes.
    let node_ids = network.add_nodes(rng, network_size).await;

    // Create `deploy_count` random deploys.
    let (all_deploy_hashes, mut deploys): (BTreeSet<_>, Vec<_>) = iter::repeat_with(|| {
        let deploy = Box::new(Deploy::random_valid_native_transfer(rng));
        (*deploy.hash(), deploy)
    })
    .take(deploy_count)
    .unzip();

    // Give each deploy to a randomly-chosen node to be gossiped.
    for deploy in deploys.drain(..) {
        let index: usize = rng.gen_range(0..network_size);
        network
            .process_injected_effect_on(&node_ids[index], announce_deploy_received(deploy, None))
            .await;
    }

    // Check every node has every deploy stored locally.
    let all_deploys_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        nodes.values().all(|runner| {
            let hashes = runner.reactor().inner().storage.get_all_deploy_hashes();
            all_deploy_hashes == hashes
        })
    };
    network.settle_on(rng, all_deploys_held, TIMEOUT).await;

    // Ensure all responders are called before dropping the network.
    network.settle(rng, QUIET_FOR, TIMEOUT).await;

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_gossip() {
    const NETWORK_SIZES: [usize; 3] = [2, 5, 20];
    const DEPLOY_COUNTS: [usize; 3] = [1, 10, 30];

    let mut rng = crate::new_rng();

    for network_size in &NETWORK_SIZES {
        for deploy_count in &DEPLOY_COUNTS {
            run_gossip(&mut rng, *network_size, *deploy_count).await
        }
    }
}

#[tokio::test]
async fn should_get_from_alternate_source() {
    const NETWORK_SIZE: usize = 3;
    const POLL_DURATION: Duration = Duration::from_millis(10);
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let mut rng = crate::new_rng();

    // Add `NETWORK_SIZE` nodes.
    let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;

    // Create random deploy.
    let deploy = Box::new(Deploy::random_valid_native_transfer(&mut rng));
    let deploy_id = *deploy.hash();

    // Give the deploy to nodes 0 and 1 to be gossiped.
    for node_id in node_ids.iter().take(2) {
        network
            .process_injected_effect_on(node_id, announce_deploy_received(deploy.clone(), None))
            .await;
    }

    // Run node 0 until it has sent the gossip request then remove it from the network.
    let made_gossip_request = |event: &Event| -> bool {
        matches!(event, Event::NetworkRequest(NetworkRequest::Gossip { .. }))
    };
    network
        .crank_until(&node_ids[0], &mut rng, made_gossip_request, TIMEOUT)
        .await;
    assert!(network.remove_node(&node_ids[0]).is_some());
    debug!("removed node {}", &node_ids[0]);

    // Run node 2 until it receives and responds to the gossip request from node 0.
    let node_id_0 = node_ids[0];
    let sent_gossip_response = move |event: &Event| -> bool {
        match event {
            Event::NetworkRequest(NetworkRequest::SendMessage { dest, payload, .. }) => {
                if let NodeMessage::DeployGossiper(Message::GossipResponse { .. }) = **payload {
                    **dest == node_id_0
                } else {
                    false
                }
            }
            _ => false,
        }
    };
    network
        .crank_until(&node_ids[2], &mut rng, sent_gossip_response, TIMEOUT)
        .await;

    // Run nodes 1 and 2 until settled.  Node 2 will be waiting for the deploy from node 0.
    network.settle(&mut rng, POLL_DURATION, TIMEOUT).await;

    // Advance time to trigger node 2's timeout causing it to request the deploy from node 1.
    let duration_to_advance = Config::default().get_remainder_timeout();
    testing::advance_time(duration_to_advance.into()).await;

    // Check node 0 has the deploy stored locally.
    let deploy_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        let runner = nodes.get(&node_ids[2]).unwrap();
        runner
            .reactor()
            .inner()
            .storage
            .get_deploy_by_hash(deploy_id)
            .map(|retrieved_deploy| retrieved_deploy == *deploy)
            .unwrap_or_default()
    };
    network.settle_on(&mut rng, deploy_held, TIMEOUT).await;

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_timeout_gossip_response() {
    const PAUSE_DURATION: Duration = Duration::from_millis(50);
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let mut rng = crate::new_rng();

    // The target number of peers to infect with a given piece of data.
    let infection_target = Config::default().infection_target();

    // Add `infection_target + 1` nodes.
    let mut node_ids = network
        .add_nodes(&mut rng, infection_target as usize + 1)
        .await;

    // Create random deploy.
    let deploy = Box::new(Deploy::random_valid_native_transfer(&mut rng));
    let deploy_id = *deploy.hash();

    // Give the deploy to node 0 to be gossiped.
    network
        .process_injected_effect_on(&node_ids[0], announce_deploy_received(deploy.clone(), None))
        .await;

    // Run node 0 until it has sent the gossip requests.
    let made_gossip_request = |event: &Event| -> bool {
        matches!(
            event,
            Event::DeployGossiper(super::Event::GossipedTo { .. })
        )
    };
    network
        .crank_until(&node_ids[0], &mut rng, made_gossip_request, TIMEOUT)
        .await;
    // Give node 0 time to set the timeouts before advancing the clock.
    time::sleep(PAUSE_DURATION).await;

    // Replace all nodes except node 0 with new nodes.
    for node_id in node_ids.drain(1..) {
        assert!(network.remove_node(&node_id).is_some());
        debug!("removed node {}", node_id);
    }
    for _ in 0..infection_target {
        let (node_id, _runner) = network.add_node(&mut rng).await.unwrap();
        node_ids.push(node_id);
    }

    // Advance time to trigger node 0's timeout causing it to gossip to the new nodes.
    let duration_to_advance = Config::default().gossip_request_timeout();
    testing::advance_time(duration_to_advance.into()).await;

    // Check every node has every deploy stored locally.
    let deploy_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        nodes.values().all(|runner| {
            runner
                .reactor()
                .inner()
                .storage
                .get_deploy_by_hash(deploy_id)
                .map(|retrieved_deploy| retrieved_deploy == *deploy)
                .unwrap_or_default()
        })
    };
    network.settle_on(&mut rng, deploy_held, TIMEOUT).await;

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_timeout_new_item_from_peer() {
    const NETWORK_SIZE: usize = 2;
    const VALIDATE_AND_STORE_TIMEOUT: Duration = Duration::from_secs(1);
    const TIMEOUT: Duration = Duration::from_secs(5);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let mut test_rng = crate::new_rng();
    let rng = &mut test_rng;

    let node_ids = network.add_nodes(rng, NETWORK_SIZE).await;
    let node_0 = node_ids[0];
    let node_1 = node_ids[1];
    // Set the timeout on node 0 low for testing.
    let reactor_0 = network
        .nodes_mut()
        .get_mut(&node_0)
        .unwrap()
        .reactor_mut()
        .inner_mut();
    reactor_0.deploy_gossiper.validate_and_store_timeout = VALIDATE_AND_STORE_TIMEOUT;
    // Switch off the fake deploy acceptor on node 0 so that once the new deploy is received, no
    // component triggers the `ItemReceived` event.
    reactor_0.fake_deploy_acceptor.set_active(false);

    let deploy = Box::new(Deploy::random_valid_native_transfer(rng));

    // Give the deploy to node 1 to gossip to node 0.
    network
        .process_injected_effect_on(&node_1, announce_deploy_received(deploy.clone(), None))
        .await;

    // Run the network until node 1 has sent the gossip request and node 0 has handled it to the
    // point where the `NewItemBody` announcement has been received).
    let got_new_item_body_announcement = |event: &Event| -> bool {
        matches!(
            event,
            Event::DeployGossiperAnnouncement(GossiperAnnouncement::NewItemBody { .. })
        )
    };
    network
        .crank_all_until(&node_0, rng, got_new_item_body_announcement, TIMEOUT)
        .await;

    // Run node 0 until it receives its own `CheckItemReceivedTimeout` event.
    let received_timeout_event = |event: &Event| -> bool {
        matches!(
            event,
            Event::DeployGossiper(super::Event::CheckItemReceivedTimeout { .. })
        )
    };
    network
        .crank_until(&node_0, rng, received_timeout_event, TIMEOUT)
        .await;

    // Ensure node 0 makes a `FinishedGossiping` announcement.
    let made_finished_gossiping_announcement = |event: &Event| -> bool {
        matches!(
            event,
            Event::DeployGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(_))
        )
    };
    network
        .crank_until(&node_0, rng, made_finished_gossiping_announcement, TIMEOUT)
        .await;

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_not_gossip_old_stored_item_again() {
    const NETWORK_SIZE: usize = 2;
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let mut test_rng = crate::new_rng();
    let rng = &mut test_rng;

    let node_ids = network.add_nodes(rng, NETWORK_SIZE).await;
    let node_0 = node_ids[0];

    let deploy = Box::new(Deploy::random_valid_native_transfer(rng));

    // Store the deploy on node 0.
    let store_deploy = |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .put_deploy_to_storage(deploy.clone())
            .ignore()
    };
    network
        .process_injected_effect_on(&node_0, store_deploy)
        .await;

    // Node 1 sends a gossip message to node 0.
    network
        .process_injected_effect_on(&node_0, |effect_builder| {
            let event = Event::DeployGossiperIncoming(GossiperIncoming {
                sender: node_ids[1],
                message: Message::Gossip(deploy.gossip_id()),
            });
            effect_builder
                .into_inner()
                .schedule(event, QueueKind::Gossip)
                .ignore()
        })
        .await;

    // Run node 0 until it has handled the gossip message and checked if the deploy is already
    // stored.
    let checked_if_stored = |event: &Event| -> bool {
        matches!(
            event,
            Event::DeployGossiper(super::Event::IsStoredResult { .. })
        )
    };
    network
        .crank_until(&node_0, rng, checked_if_stored, TIMEOUT)
        .await;
    // Assert the message did not cause a new entry in the gossip table and spawned no new events.
    assert!(network
        .nodes()
        .get(&node_0)
        .unwrap()
        .reactor()
        .inner()
        .deploy_gossiper
        .table
        .is_empty());
    assert!(matches!(
        network.crank(&node_0, rng).await,
        TryCrankOutcome::NoEventsToProcess
    ));

    NetworkController::<NodeMessage>::remove_active();
}

enum Unexpected {
    Response,
    GetItem,
    Item,
}

async fn should_ignore_unexpected_message(message_type: Unexpected) {
    const NETWORK_SIZE: usize = 2;
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let mut test_rng = crate::new_rng();
    let rng = &mut test_rng;

    let node_ids = network.add_nodes(rng, NETWORK_SIZE).await;
    let node_0 = node_ids[0];

    let deploy = Box::new(Deploy::random_valid_native_transfer(rng));

    let message = match message_type {
        Unexpected::Response => Message::GossipResponse {
            item_id: deploy.gossip_id(),
            is_already_held: false,
        },
        Unexpected::GetItem => Message::GetItem(deploy.gossip_id()),
        Unexpected::Item => Message::Item(deploy),
    };

    // Node 1 sends an unexpected message to node 0.
    network
        .process_injected_effect_on(&node_0, |effect_builder| {
            let event = Event::DeployGossiperIncoming(GossiperIncoming {
                sender: node_ids[1],
                message,
            });
            effect_builder
                .into_inner()
                .schedule(event, QueueKind::Gossip)
                .ignore()
        })
        .await;

    // Run node 0 until it has handled the gossip message.
    let received_gossip_message =
        |event: &Event| -> bool { matches!(event, Event::DeployGossiperIncoming(..)) };
    network
        .crank_until(&node_0, rng, received_gossip_message, TIMEOUT)
        .await;
    // Assert the message did not cause a new entry in the gossip table and spawned no new events.
    assert!(network
        .nodes()
        .get(&node_0)
        .unwrap()
        .reactor()
        .inner()
        .deploy_gossiper
        .table
        .is_empty());
    assert!(matches!(
        network.crank(&node_0, rng).await,
        TryCrankOutcome::NoEventsToProcess
    ));

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_ignore_unexpected_response_message() {
    should_ignore_unexpected_message(Unexpected::Response).await
}

#[tokio::test]
async fn should_ignore_unexpected_get_item_message() {
    should_ignore_unexpected_message(Unexpected::GetItem).await
}

#[tokio::test]
async fn should_ignore_unexpected_item_message() {
    should_ignore_unexpected_message(Unexpected::Item).await
}
