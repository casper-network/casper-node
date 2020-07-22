#![cfg(test)]
use std::fmt::{self, Debug, Display, Formatter};

use derive_more::From;
use futures::future::FutureExt;
use prometheus::Registry;
use tempfile::TempDir;
use thiserror::Error;

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

#[tokio::test]
async fn should_fetch_from_peer() {
    const NETWORK_SIZE: usize = 2;
    const TIMEOUT: Duration = Duration::from_secs(1);

    NetworkController::<Message>::create_active();
    let mut network = Network::<Reactor>::new();
    let mut rng = rand::thread_rng();

    // Add two nodes.
    let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;

    // Create a random deploy.
    let deploy: Deploy = rng.gen();
    let deploy_hash = *deploy.id();

    // Store the deploy on node 0.
    network
        .process_injected_effect_on(&node_ids[0], put_deploy_to_storage(deploy.clone()))
        .await;
    let stored_deploy = move |event: &Event| -> bool {
        match event {
            Event::StorageAnnouncement(StorageAnnouncement::StoredDeploy { .. }) => true,
            _ => false,
        }
    };
    network
        .crank_until(&node_ids[0], &mut rng, stored_deploy, TIMEOUT)
        .await;

    // Try to fetch the deploy from node 1.  It should fail to get it from its own storage component
    // but provide it by getting it from node 0.
    network
        .process_injected_effect_on(&node_ids[1], fetch_deploy(deploy_hash, node_ids[0]))
        .await;
    let deploy_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        let runner = nodes.get(&node_ids[1]).unwrap();
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
    network.settle_on(&mut rng, deploy_held, TIMEOUT).await;

    NetworkController::<Message>::remove_active();
}
