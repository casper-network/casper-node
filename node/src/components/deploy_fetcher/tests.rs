#![cfg(test)]
use std::fmt::{self, Debug, Display, Formatter};

use derive_more::From;
use futures::future::FutureExt;
use prometheus::Registry;
use rand::rngs::ThreadRng;
use tempfile::TempDir;
use thiserror::Error;
use tokio::time;

use super::*;
use crate::{
    components::{
        in_memory_network::{InMemoryNetwork, NetworkController, NodeId},
        storage::{self, Storage, StorageType},
    },
    effect::{
        announcements::{NetworkAnnouncement, StorageAnnouncement},
        requests::DeployFetcherRequest,
    },
    reactor::{self, EventQueueHandle, Runner},
    testing::{
        network::{Network, NetworkedReactor},
        ConditionCheckReactor,
    },
    types::Deploy,
};

const TIMEOUT: Duration = Duration::from_secs(1);

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
enum Event {
    #[from]
    Storage(StorageRequest<Storage>),
    #[from]
    DeployFetcher(super::Event),
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),
    #[from]
    DeployFetcherRequest(DeployFetcherRequest<NodeId>),
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
    #[from]
    StorageAnnouncement(StorageAnnouncement<Storage>),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::DeployFetcher(event) => write!(formatter, "deploy fetcher: {}", event),
            Event::NetworkRequest(req) => write!(formatter, "network request: {}", req),
            Event::DeployFetcherRequest(req) => {
                write!(formatter, "deploy fetcher request: {}", req)
            }
            Event::NetworkAnnouncement(ann) => write!(formatter, "network announcement: {}", ann),
            Event::StorageAnnouncement(ann) => write!(formatter, "storage announcement: {}", ann),
        }
    }
}

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
enum Error {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
    #[error("storage error: {0}")]
    Storage(#[from] storage::Error),
}

struct Reactor {
    network: InMemoryNetwork<Message>,
    storage: Storage,
    deploy_fetcher: DeployFetcher,
    _storage_tempdir: TempDir,
}

impl Drop for Reactor {
    fn drop(&mut self) {
        NetworkController::<Message>::remove_node(&self.network.node_id())
    }
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = GossipConfig;
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

        let deploy_fetcher = DeployFetcher::new(config);

        let reactor = Reactor {
            network,
            storage,
            deploy_fetcher,
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
            Event::DeployFetcher(event) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher.handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequest(request) => reactor::wrap_effects(
                Event::NetworkRequest,
                self.network.handle_event(effect_builder, rng, request),
            ),
            Event::DeployFetcherRequest(request) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher
                    .handle_event(effect_builder, rng, request.into()),
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
                    self.deploy_fetcher.handle_event(effect_builder, rng, event),
                )
            }
            Event::StorageAnnouncement(_) => Effects::new(),
        }
    }
}

impl NetworkedReactor for Reactor {
    type NodeId = NodeId;

    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn put_deploy_to_storage(deploy: Deploy) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| effect_builder.put_deploy_to_storage(deploy).ignore()
}

fn fetch_deploy(
    deploy_hash: DeployHash,
    node_id: NodeId,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    move |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .fetch_deploy(deploy_hash, node_id)
            .then(move |maybe_deploy| async move {
                assert!(maybe_deploy.is_some());
            })
            .ignore()
    }
}

/// Store a deploy on a target node.
async fn store_deploy(
    deploy: &Deploy,
    node_id: &NodeId,
    network: &mut Network<Reactor>,
    mut rng: &mut ThreadRng,
) {
    network
        .process_injected_effect_on(node_id, put_deploy_to_storage(deploy.clone()))
        .await;

    // cycle to storage
    network
        .crank_until(
            node_id,
            &mut rng,
            move |event: &Event| -> bool {
                match event {
                    Event::StorageAnnouncement(StorageAnnouncement::StoredNewDeploy { .. }) => true,
                    _ => false,
                }
            },
            TIMEOUT,
        )
        .await;
}

async fn assert_settled(
    node_id: &NodeId,
    deploy_hash: DeployHash,
    network: &mut Network<Reactor>,
    mut rng: ThreadRng,
    timeout: Duration,
    allow_timeout: bool,
) {
    let deploy_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        let runner = nodes.get(node_id).unwrap();
        runner
            .reactor()
            .inner()
            .storage
            .deploy_store()
            .get(smallvec![deploy_hash])
            .pop()
            .expect("should only be a single result")
            .is_ok()
    };

    if allow_timeout {
        time::timeout(
            timeout,
            network.settle_on_indefinitely(&mut rng, deploy_held),
        )
        .await
        .unwrap_or_default();
    } else {
        // Panics internally if unsuccessful.
        network.settle_on(&mut rng, deploy_held, timeout).await;
    }
}

#[tokio::test]
async fn should_fetch_from_local() {
    const NETWORK_SIZE: usize = 1;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = Network::<Reactor>::new();
        let mut rng = rand::thread_rng();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random deploy.
    let deploy: Deploy = rng.gen();

    // Store deploy on a node.
    let node_to_store_on = &node_ids[0];
    store_deploy(&deploy, node_to_store_on, &mut network, &mut rng).await;

    // Try to fetch the deploy from a node that holds it.
    let node_id = &node_ids[0];
    let deploy_hash = *deploy.id();
    network
        .process_injected_effect_on(node_id, fetch_deploy(deploy_hash, *node_id))
        .await;

    assert_settled(node_id, *deploy.id(), &mut network, rng, TIMEOUT, false).await;

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_fetch_from_peer() {
    const NETWORK_SIZE: usize = 2;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = Network::<Reactor>::new();
        let mut rng = rand::thread_rng();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random deploy.
    let deploy: Deploy = rng.gen();

    // Store deploy on a node.
    let node_to_store_on = &node_ids[0];
    store_deploy(&deploy, node_to_store_on, &mut network, &mut rng).await;

    let node_id = &node_ids[0];
    let peer = node_ids[1];
    let deploy_hash = *deploy.id();

    // Try to fetch the deploy from a node that does not hold it; should get from peer.
    network
        .process_injected_effect_on(node_id, fetch_deploy(deploy_hash, peer))
        .await;

    assert_settled(node_id, *deploy.id(), &mut network, rng, TIMEOUT, false).await;

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_timeout_fetch_from_peer() {
    const NETWORK_SIZE: usize = 2;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = Network::<Reactor>::new();
        let mut rng = rand::thread_rng();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random deploy.
    let deploy: Deploy = rng.gen();
    let deploy_hash = deploy.id();

    let holding_node = node_ids[0];
    let requesting_node = node_ids[1];

    // Store deploy on holding node.
    store_deploy(&deploy, &holding_node, &mut network, &mut rng).await;

    // Initiate requesting node asking for deploy from holding node.
    network
        .process_injected_effect_on(
            &requesting_node,
            move |effect_builder: EffectBuilder<Event>| {
                effect_builder
                    .fetch_deploy(*deploy_hash, holding_node)
                    .then(move |maybe_deploy| async move {
                        // This is the final assert; we expect the request to time out
                        // so this should be None on the requesting node.
                        assert!(maybe_deploy.is_none());
                    })
                    .ignore()
            },
        )
        .await;

    // Crank until message sent from the requestor.
    network
        .crank_until(
            &requesting_node,
            &mut rng,
            move |event: &Event| -> bool {
                match event {
                    Event::NetworkRequest(request) => match request {
                        NetworkRequest::SendMessage { payload, .. } => match payload {
                            Message::GetRequest(_) => true,
                            _ => false,
                        },
                        _ => false,
                    },
                    _ => false,
                }
            },
            TIMEOUT,
        )
        .await;

    // Crank until the message is received by the holding node.
    network
        .crank_until(
            &holding_node,
            &mut rng,
            move |event: &Event| -> bool {
                match event {
                    Event::NetworkRequest(request) => match request {
                        NetworkRequest::SendMessage { payload, .. } => match payload {
                            Message::GetResponse(_) => true,
                            _ => false,
                        },
                        _ => false,
                    },
                    _ => false,
                }
            },
            TIMEOUT,
        )
        .await;

    // Advance time.
    let secs_to_advance = GossipConfig::default().get_remainder_timeout_secs();
    time::pause();
    time::advance(Duration::from_secs(secs_to_advance)).await;
    time::resume();

    // Settle the network, allowing timeout to avoid panic.
    assert_settled(
        &requesting_node,
        *deploy.id(),
        &mut network,
        rng,
        TIMEOUT,
        true,
    )
    .await;

    NetworkController::<Message>::remove_active();
}
