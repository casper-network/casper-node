#![cfg(test)]
use std::{
    collections::{BTreeSet, HashMap},
    fmt::{self, Debug, Display, Formatter},
    iter,
};

use derive_more::From;
use prometheus::Registry;
use smallvec::smallvec;
use tempfile::TempDir;
use thiserror::Error;
use tokio::time;
use tracing::debug;

use super::*;
use crate::{
    components::{
        in_memory_network::{InMemoryNetwork, NetworkController, NodeId},
        storage::{self, Storage, StorageType},
    },
    effect::announcements::{ApiServerAnnouncement, NetworkAnnouncement, StorageAnnouncement},
    reactor::{self, EventQueueHandle, Runner},
    testing::{
        network::{Network, NetworkedReactor},
        ConditionCheckReactor,
    },
    types::Deploy,
};

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
enum Event {
    #[from]
    /// Storage event.
    Storage(StorageRequest<Storage>),
    /// Deploy gossiper event.
    #[from]
    DeployGossiper(super::Event),
    /// Network request.
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),
    /// Network announcement.
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
    /// Storage announcement.
    #[from]
    StorageAnnouncement(StorageAnnouncement<Storage>),
    /// API server announcement.
    #[from]
    ApiServerAnnouncement(ApiServerAnnouncement),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::DeployGossiper(event) => write!(formatter, "deploy gossiper: {}", event),
            Event::NetworkRequest(req) => write!(formatter, "network request: {}", req),
            Event::NetworkAnnouncement(ann) => write!(formatter, "network announcement: {}", ann),
            Event::StorageAnnouncement(ann) => write!(formatter, "storage announcement: {}", ann),
            Event::ApiServerAnnouncement(ann) => {
                write!(formatter, "api server announcement: {}", ann)
            }
        }
    }
}

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
pub enum Error {
    /// Metrics-related error
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
    /// `Storage` component error.
    #[error("storage error: {0}")]
    Storage(#[from] storage::Error),
}
struct Reactor {
    network: InMemoryNetwork<Message>,
    storage: Storage,
    deploy_gossiper: DeployGossiper,
    _storage_tempdir: TempDir,
}

impl Drop for Reactor {
    fn drop(&mut self) {
        NetworkController::<Message>::remove_node(&self.network.node_id())
    }
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = GossipTableConfig;
    type Error = Error;

    fn new<R: Rng + ?Sized>(
        config: Self::Config,
        _registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut R,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let network = NetworkController::create_node(event_queue, rng);

        let (storage_config, _storage_tempdir) = storage::Config::default_for_tests();
        let storage = Storage::new(&storage_config)?;

        let deploy_gossiper = DeployGossiper::new(config);

        let reactor = Reactor {
            network,
            storage,
            deploy_gossiper,
            _storage_tempdir,
        };

        let effects = Effects::new();

        Ok((reactor, effects))
    }

    fn dispatch_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut R,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::DeployGossiper(event) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequest(request) => reactor::wrap_effects(
                Event::NetworkRequest,
                self.network.handle_event(effect_builder, rng, request),
            ),
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => {
                let event = super::Event::MessageReceived {
                    sender,
                    message: payload,
                };
                reactor::wrap_effects(
                    From::from,
                    self.deploy_gossiper
                        .handle_event(effect_builder, rng, event),
                )
            }
            Event::StorageAnnouncement(_) => Effects::new(),
            Event::ApiServerAnnouncement(ApiServerAnnouncement::DeployReceived { deploy }) => {
                let event = super::Event::DeployReceived { deploy };
                self.dispatch_event(effect_builder, rng, Event::DeployGossiper(event))
            }
        }
    }
}

impl NetworkedReactor for Reactor {
    type NodeId = NodeId;

    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn create_deploy_received(
    deploy: Box<Deploy>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| effect_builder.announce_deploy_received(deploy).ignore()
}

#[tokio::test]
async fn should_gossip() {
    const NETWORK_SIZE_MIN: usize = 2;
    const NETWORK_SIZE_MAX: usize = 20;
    const DEPLOY_COUNT_MIN: usize = 1;
    const DEPLOY_COUNT_MAX: usize = 30;
    const TIMEOUT: Duration = Duration::from_secs(20);

    NetworkController::<Message>::create_active();
    let mut network = Network::<Reactor>::new();
    let mut rng = rand::thread_rng();

    // Add `network_size` nodes.
    let network_size = rng.gen_range(NETWORK_SIZE_MIN, NETWORK_SIZE_MAX + 1);
    let node_ids = network.add_nodes(&mut rng, network_size).await;

    // Create `deploy_count` random deploys.
    let deploy_count: usize = rng.gen_range(DEPLOY_COUNT_MIN, DEPLOY_COUNT_MAX + 1);
    let (all_deploy_hashes, mut deploys): (BTreeSet<_>, Vec<_>) = iter::repeat_with(|| {
        let deploy = Box::new(rng.gen::<Deploy>());
        (*deploy.id(), deploy)
    })
    .take(deploy_count)
    .unzip();

    // Give each deploy to a randomly-chosen node to be gossiped.
    for deploy in deploys.drain(..) {
        let index: usize = rng.gen_range(0, network_size);
        network
            .process_injected_effect_on(&node_ids[index], create_deploy_received(deploy))
            .await;
    }

    // Check every node has every deploy stored locally.
    let all_deploys_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        nodes.values().all(|runner| {
            let hashes = runner
                .reactor()
                .inner()
                .storage
                .deploy_store()
                .ids()
                .unwrap()
                .into_iter()
                .collect();
            all_deploy_hashes == hashes
        })
    };
    network.settle_on(&mut rng, all_deploys_held, TIMEOUT).await;

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_get_from_alternate_source() {
    const NETWORK_SIZE: usize = 3;
    const POLL_DURATION: Duration = Duration::from_millis(10);
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<Message>::create_active();
    let mut network = Network::<Reactor>::new();
    let mut rng = rand::thread_rng();

    // Add `NETWORK_SIZE` nodes.
    let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;

    // Create random deploy.
    let deploy = Box::new(rng.gen::<Deploy>());
    let deploy_id = *deploy.id();

    // Give the deploy to nodes 0 and 1 to be gossiped.
    for node_id in node_ids.iter().take(2) {
        network
            .process_injected_effect_on(&node_id, create_deploy_received(deploy.clone()))
            .await;
    }

    // Run node 0 until it has sent the gossip request then remove it from the network.
    let made_gossip_request = |event: &Event| -> bool {
        match event {
            Event::NetworkRequest(NetworkRequest::Gossip { .. }) => true,
            _ => false,
        }
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
            Event::NetworkRequest(NetworkRequest::SendMessage {
                dest,
                payload: Message::GossipResponse { .. },
                ..
            }) => dest == &node_id_0,
            _ => false,
        }
    };
    network
        .crank_until(&node_ids[2], &mut rng, sent_gossip_response, TIMEOUT)
        .await;

    // Run nodes 1 and 2 until settled.  Node 2 will be waiting for the deploy from node 0.
    network.settle(&mut rng, POLL_DURATION, TIMEOUT).await;

    // Advance time to trigger node 2's timeout causing it to request the deploy from node 1.
    let secs_to_advance = GossipTableConfig::default().get_remainder_timeout_secs();
    time::pause();
    time::advance(Duration::from_secs(secs_to_advance)).await;
    time::resume();
    debug!("advanced time by {} secs", secs_to_advance);

    // Check node 0 has the deploy stored locally.
    let deploy_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        let runner = nodes.get(&node_ids[2]).unwrap();
        runner
            .reactor()
            .inner()
            .storage
            .deploy_store()
            .get(smallvec![deploy_id])
            .pop()
            .expect("should only be a single result")
            .map(|retrieved_deploy| retrieved_deploy == *deploy)
            .unwrap_or_default()
    };
    network.settle_on(&mut rng, deploy_held, TIMEOUT).await;

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_timeout_gossip_response() {
    const PAUSE_DURATION: Duration = Duration::from_millis(50);
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<Message>::create_active();
    let mut network = Network::<Reactor>::new();
    let mut rng = rand::thread_rng();

    // The target number of peers to infect with a given piece of data.
    let infection_target = GossipTableConfig::default().infection_target();

    // Add `infection_target + 1` nodes.
    let mut node_ids = network
        .add_nodes(&mut rng, infection_target as usize + 1)
        .await;

    // Create random deploy.
    let deploy = Box::new(rng.gen::<Deploy>());
    let deploy_id = *deploy.id();

    // Give the deploy to node 0 to be gossiped.
    network
        .process_injected_effect_on(&node_ids[0], create_deploy_received(deploy.clone()))
        .await;

    // Run node 0 until it has sent the gossip requests.
    let made_gossip_request = |event: &Event| -> bool {
        match event {
            Event::DeployGossiper(super::Event::GossipedTo { .. }) => true,
            _ => false,
        }
    };
    network
        .crank_until(&node_ids[0], &mut rng, made_gossip_request, TIMEOUT)
        .await;
    // Give node 0 time to set the timeouts before advancing the clock.
    time::delay_for(PAUSE_DURATION).await;

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
    let secs_to_advance = GossipTableConfig::default().gossip_request_timeout_secs();
    time::pause();
    time::advance(Duration::from_secs(secs_to_advance)).await;
    time::resume();
    debug!("advanced time by {} secs", secs_to_advance);

    // Check every node has every deploy stored locally.
    let deploy_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        nodes.values().all(|runner| {
            runner
                .reactor()
                .inner()
                .storage
                .deploy_store()
                .get(smallvec![deploy_id])
                .pop()
                .expect("should only be a single result")
                .map(|retrieved_deploy| retrieved_deploy == *deploy)
                .unwrap_or_default()
        })
    };
    network.settle_on(&mut rng, deploy_held, TIMEOUT).await;

    NetworkController::<Message>::remove_active();
}
