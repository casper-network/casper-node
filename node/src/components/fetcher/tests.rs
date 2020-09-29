#![cfg(test)]
use std::{
    fmt::{self, Debug, Display, Formatter},
    sync::{Arc, Mutex},
};

use derive_more::From;
use futures::FutureExt;
use prometheus::Registry;
use tempfile::TempDir;
use thiserror::Error;
use tokio::time;

use super::*;
use crate::{
    components::{
        chainspec_loader::Chainspec,
        deploy_acceptor::{self, DeployAcceptor},
        in_memory_network::{InMemoryNetwork, NetworkController, NodeId},
        storage::{self, Storage, StorageType},
    },
    effect::{
        announcements::{ApiServerAnnouncement, DeployAcceptorAnnouncement, NetworkAnnouncement},
        requests::FetcherRequest,
    },
    protocol::Message,
    reactor::{self, EventQueueHandle, Runner},
    testing::{
        network::{Network, NetworkedReactor},
        ConditionCheckReactor, TestRng,
    },
    types::{Deploy, DeployHash, Tag},
    utils::{Loadable, WithDir},
};

const TIMEOUT: Duration = Duration::from_secs(1);

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
enum Event {
    #[from]
    Storage(storage::Event<Storage>),
    #[from]
    DeployAcceptor(deploy_acceptor::Event),
    #[from]
    DeployFetcher(super::Event<Deploy>),
    #[from]
    DeployFetcherRequest(FetcherRequest<NodeId, Deploy>),
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
    #[from]
    ApiServerAnnouncement(ApiServerAnnouncement),
    #[from]
    DeployAcceptorAnnouncement(DeployAcceptorAnnouncement<NodeId>),
}

impl From<StorageRequest<Storage>> for Event {
    fn from(request: StorageRequest<Storage>) -> Self {
        Event::Storage(storage::Event::Request(request))
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::DeployAcceptor(event) => write!(formatter, "deploy acceptor: {}", event),
            Event::DeployFetcher(event) => write!(formatter, "fetcher: {}", event),
            Event::NetworkRequest(req) => write!(formatter, "network request: {}", req),
            Event::DeployFetcherRequest(req) => write!(formatter, "fetcher request: {}", req),
            Event::NetworkAnnouncement(ann) => write!(formatter, "network announcement: {}", ann),
            Event::ApiServerAnnouncement(ann) => {
                write!(formatter, "api server announcement: {}", ann)
            }
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(formatter, "deploy-acceptor announcement: {}", ann)
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

struct Reactor {
    network: InMemoryNetwork<Message>,
    storage: Storage,
    deploy_acceptor: DeployAcceptor,
    deploy_fetcher: Fetcher<Deploy>,
    _storage_tempdir: TempDir,
}

impl Drop for Reactor {
    fn drop(&mut self) {
        NetworkController::<Message>::remove_node(&self.network.node_id())
    }
}

impl reactor::Reactor<TestRng> for Reactor {
    type Event = Event;
    type Config = GossipConfig;
    type Error = Error;

    fn new(
        config: Self::Config,
        _registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut TestRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let network = NetworkController::create_node(event_queue, rng);

        let (storage_config, _storage_tempdir) = storage::Config::default_for_tests();
        let storage = Storage::new(WithDir::new(_storage_tempdir.path(), storage_config)).unwrap();

        let deploy_acceptor = DeployAcceptor::new();
        let deploy_fetcher = Fetcher::<Deploy>::new(config);

        let reactor = Reactor {
            network,
            storage,
            deploy_acceptor,
            deploy_fetcher,
            _storage_tempdir,
        };

        let effects = Effects::new();

        Ok((reactor, effects))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut TestRng,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Storage(storage::Event::Request(StorageRequest::GetChainspec {
                responder,
                ..
            })) => responder
                .respond(Some(Chainspec::from_resources("local/chainspec.toml")))
                .ignore(),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::DeployAcceptor(event) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::DeployFetcher(deploy_event) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher
                    .handle_event(effect_builder, rng, deploy_event),
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
                let reactor_event = match payload {
                    Message::GetRequest {
                        tag: Tag::Deploy,
                        serialized_id,
                    } => {
                        let deploy_hash = match rmp_serde::from_read_ref(&serialized_id) {
                            Ok(hash) => hash,
                            Err(error) => {
                                error!(
                                    "failed to decode {:?} from {}: {}",
                                    serialized_id, sender, error
                                );
                                return Effects::new();
                            }
                        };
                        Event::Storage(storage::Event::GetDeployForPeer {
                            deploy_hash,
                            peer: sender,
                        })
                    }
                    Message::GetResponse {
                        tag: Tag::Deploy,
                        serialized_item,
                    } => {
                        let deploy = match rmp_serde::from_read_ref(&serialized_item) {
                            Ok(deploy) => Box::new(deploy),
                            Err(error) => {
                                error!("failed to decode deploy from {}: {}", sender, error);
                                return Effects::new();
                            }
                        };
                        Event::DeployAcceptor(deploy_acceptor::Event::Accept {
                            deploy,
                            source: Source::Peer(sender),
                        })
                    }
                    msg => panic!("should not get {}", msg),
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::NetworkAnnouncement(ann) => {
                unreachable!("should not receive announcements of type {:?}", ann);
            }
            Event::ApiServerAnnouncement(ApiServerAnnouncement::DeployReceived { deploy }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::<NodeId>::Client,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployAcceptor(event))
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::AcceptedNewDeploy {
                deploy,
                source,
            }) => {
                let event = super::Event::GotRemotely {
                    item: deploy,
                    source,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployFetcher(event))
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                deploy: _,
                source: _,
            }) => Effects::new(),
        }
    }
}

impl NetworkedReactor for Reactor {
    type NodeId = NodeId;

    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn announce_deploy_received(deploy: Deploy) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .announce_deploy_received(Box::new(deploy))
            .ignore()
    }
}

fn fetch_deploy(
    deploy_hash: DeployHash,
    node_id: NodeId,
    fetched: Arc<Mutex<(bool, Option<FetchResult<Deploy>>)>>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    move |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .fetch_deploy(deploy_hash, node_id)
            .then(move |maybe_deploy| async move {
                let mut result = fetched.lock().unwrap();
                result.0 = true;
                result.1 = maybe_deploy;
            })
            .ignore()
    }
}

/// Store a deploy on a target node.
async fn store_deploy(
    deploy: &Deploy,
    node_id: &NodeId,
    network: &mut Network<Reactor>,
    mut rng: &mut TestRng,
) {
    network
        .process_injected_effect_on(node_id, announce_deploy_received(deploy.clone()))
        .await;

    // cycle to deploy acceptor announcement
    network
        .crank_until(
            node_id,
            &mut rng,
            move |event: &Event| -> bool {
                match event {
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy { .. },
                    ) => true,
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
    expected_result: Option<FetchResult<Deploy>>,
    fetched: Arc<Mutex<(bool, Option<FetchResult<Deploy>>)>>,
    network: &mut Network<Reactor>,
    rng: &mut TestRng,
    timeout: Duration,
) {
    let has_responded = |_nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>, _>>| {
        fetched.lock().unwrap().0
    };

    network.settle_on(rng, has_responded, timeout).await;

    let maybe_stored_deploy = network
        .nodes()
        .get(node_id)
        .unwrap()
        .reactor()
        .inner()
        .storage
        .deploy_store()
        .get(smallvec![deploy_hash])
        .pop()
        .expect("should only be a single result")
        .expect("should not error while getting");
    assert_eq!(expected_result.is_some(), maybe_stored_deploy.is_some());

    assert_eq!(fetched.lock().unwrap().1, expected_result)
}

#[tokio::test]
async fn should_fetch_from_local() {
    const NETWORK_SIZE: usize = 1;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = Network::<Reactor>::new();
        let mut rng = TestRng::new();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random deploy.
    let deploy = Deploy::random(&mut rng);

    // Store deploy on a node.
    let node_to_store_on = &node_ids[0];
    store_deploy(&deploy, node_to_store_on, &mut network, &mut rng).await;

    // Try to fetch the deploy from a node that holds it.
    let node_id = &node_ids[0];
    let deploy_hash = *deploy.id();
    let fetched = Arc::new(Mutex::new((false, None)));
    network
        .process_injected_effect_on(
            node_id,
            fetch_deploy(deploy_hash, *node_id, Arc::clone(&fetched)),
        )
        .await;

    let expected_result = Some(FetchResult::FromStorage(Box::new(deploy)));
    assert_settled(
        node_id,
        deploy_hash,
        expected_result,
        fetched,
        &mut network,
        &mut rng,
        TIMEOUT,
    )
    .await;

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_fetch_from_peer() {
    const NETWORK_SIZE: usize = 2;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = Network::<Reactor>::new();
        let mut rng = TestRng::new();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random deploy.
    let deploy = Deploy::random(&mut rng);

    // Store deploy on a node.
    let node_with_deploy = &node_ids[0];
    store_deploy(&deploy, node_with_deploy, &mut network, &mut rng).await;

    let node_without_deploy = &node_ids[1];
    let deploy_hash = *deploy.id();
    let fetched = Arc::new(Mutex::new((false, None)));

    // Try to fetch the deploy from a node that does not hold it; should get from peer.
    network
        .process_injected_effect_on(
            node_without_deploy,
            fetch_deploy(deploy_hash, *node_with_deploy, Arc::clone(&fetched)),
        )
        .await;

    let expected_result = Some(FetchResult::FromPeer(Box::new(deploy), *node_with_deploy));
    assert_settled(
        node_without_deploy,
        deploy_hash,
        expected_result,
        fetched,
        &mut network,
        &mut rng,
        TIMEOUT,
    )
    .await;

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_timeout_fetch_from_peer() {
    const NETWORK_SIZE: usize = 2;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = Network::<Reactor>::new();
        let mut rng = TestRng::new();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random deploy.
    let deploy = Deploy::random(&mut rng);
    let deploy_hash = *deploy.id();

    let holding_node = node_ids[0];
    let requesting_node = node_ids[1];

    // Store deploy on holding node.
    store_deploy(&deploy, &holding_node, &mut network, &mut rng).await;

    // Initiate requesting node asking for deploy from holding node.
    let fetched = Arc::new(Mutex::new((false, None)));
    network
        .process_injected_effect_on(
            &requesting_node,
            fetch_deploy(deploy_hash, holding_node, Arc::clone(&fetched)),
        )
        .await;

    // Crank until message sent from the requester.
    network
        .crank_until(
            &requesting_node,
            &mut rng,
            move |event: &Event| -> bool {
                if let Event::NetworkRequest(NetworkRequest::SendMessage {
                    payload: Message::GetRequest { .. },
                    ..
                }) = event
                {
                    true
                } else {
                    false
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
                if let Event::NetworkRequest(NetworkRequest::SendMessage {
                    payload: Message::GetResponse { .. },
                    ..
                }) = event
                {
                    true
                } else {
                    false
                }
            },
            TIMEOUT,
        )
        .await;

    // Advance time.
    let secs_to_advance = GossipConfig::default().get_remainder_timeout_secs();
    time::pause();
    time::advance(Duration::from_secs(secs_to_advance + 10)).await;
    time::resume();

    // Settle the network, allowing timeout to avoid panic.
    let expected_result = None;
    assert_settled(
        &requesting_node,
        deploy_hash,
        expected_result,
        fetched,
        &mut network,
        &mut rng,
        TIMEOUT,
    )
    .await;

    NetworkController::<Message>::remove_active();
}
