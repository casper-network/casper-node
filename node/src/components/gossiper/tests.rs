// Unrestricted event size is okay in tests.
#![allow(clippy::large_enum_variant)]
#![cfg(test)]
use std::{
    collections::{BTreeSet, HashMap},
    iter,
    sync::Arc,
};

use derive_more::{Display, From};
use num_rational::Ratio;
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
    reactor::{self, EventQueueHandle, Runner},
    testing::{
        self,
        network::{NetworkedReactor, TestingNetwork},
        ConditionCheckReactor, FakeDeployAcceptor,
    },
    types::{Chainspec, ChainspecRawBytes, Deploy, FinalitySignature, NodeId},
    utils::WithDir,
    NodeRng,
};

const RECENT_ERA_COUNT: u64 = 5;
const MAX_TTL: TimeDiff = TimeDiff::from_seconds(86400);

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
    deploy_gossiper: Gossiper<Deploy, Event>,
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
            Ratio::new(1, 3),
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            "test",
            MAX_TTL,
            RECENT_ERA_COUNT,
            Some(registry),
        )
        .unwrap();

        let fake_deploy_acceptor = FakeDeployAcceptor::new();
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config,
            get_deploy_from_storage::<Deploy, Event>,
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
            Event::DeployGossiper(event) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
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
                    item_id: deploy.id(),
                    source,
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
