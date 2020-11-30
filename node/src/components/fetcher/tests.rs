// #![cfg(test)]
// use std::sync::{Arc, Mutex};

// use casper_node_macros::reactor;
// use futures::FutureExt;
// <<<<<<< HEAD
// =======
// use prometheus::Registry;
// use serde::Serialize;
// >>>>>>> upstream/master
// use tempfile::TempDir;
// use thiserror::Error;
// use tokio::time;

// use super::*;
// use crate::{
//     components::{
// <<<<<<< HEAD
//         deploy_acceptor,
//         in_memory_network::{NetworkController, NodeId},
//         storage::{self, Storage, StorageType},
//     },
//     effect::announcements::{DeployAcceptorAnnouncement, NetworkAnnouncement},
// =======
//         chainspec_loader::Chainspec,
//         deploy_acceptor::{self, DeployAcceptor},
//         in_memory_network::{InMemoryNetwork, NetworkController},
//         storage::{self, Storage},
//     },
//     effect::{
//         announcements::{DeployAcceptorAnnouncement, NetworkAnnouncement, RpcServerAnnouncement},
//         requests::FetcherRequest,
//     },
// >>>>>>> upstream/master
//     protocol::Message,
//     reactor::{Reactor as ReactorTrait, Runner},
//     testing::{
//         network::{Network, NetworkedReactor},
//         ConditionCheckReactor, TestRng,
//     },
// <<<<<<< HEAD
//     types::{Deploy, DeployHash},
//     utils::WithDir,
// =======
//     types::{Deploy, DeployHash, NodeId, Tag},
//     utils::{Loadable, WithDir},
//     FetcherConfig, NodeRng,
// >>>>>>> upstream/master
// };

// const TIMEOUT: Duration = Duration::from_secs(1);

// <<<<<<< HEAD
// =======
// /// Top-level event for the reactor.
// #[derive(Debug, From, Serialize)]
// #[must_use]
// enum Event {
//     #[from]
//     Storage(#[serde(skip_serializing)] storage::Event),
//     #[from]
//     DeployAcceptor(#[serde(skip_serializing)] deploy_acceptor::Event),
//     #[from]
//     DeployFetcher(#[serde(skip_serializing)] super::Event<Deploy>),
//     #[from]
//     DeployFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, Deploy>),
//     #[from]
//     NetworkRequest(#[serde(skip_serializing)] NetworkRequest<NodeId, Message>),
//     #[from]
//     LinearChainRequest(#[serde(skip_serializing)] LinearChainRequest<NodeId>),
//     #[from]
//     NetworkAnnouncement(#[serde(skip_serializing)] NetworkAnnouncement<NodeId, Message>),
//     #[from]
//     RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
//     #[from]
//     DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement<NodeId>),
// }

// impl From<StorageRequest> for Event {
//     fn from(request: StorageRequest) -> Self {
//         Event::Storage(storage::Event::from(request))
//     }
// }

// impl Display for Event {
//     fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
//         match self {
//             Event::Storage(event) => write!(formatter, "storage: {}", event),
//             Event::DeployAcceptor(event) => write!(formatter, "deploy acceptor: {}", event),
//             Event::DeployFetcher(event) => write!(formatter, "fetcher: {}", event),
//             Event::NetworkRequest(req) => write!(formatter, "network request: {}", req),
//             Event::DeployFetcherRequest(req) => write!(formatter, "fetcher request: {}", req),
//             Event::NetworkAnnouncement(ann) => write!(formatter, "network announcement: {}",
// ann),             Event::RpcServerAnnouncement(ann) => {
//                 write!(formatter, "api server announcement: {}", ann)
//             }
//             Event::DeployAcceptorAnnouncement(ann) => {
//                 write!(formatter, "deploy-acceptor announcement: {}", ann)
//             }
//             Event::LinearChainRequest(req) => write!(formatter, "linear chain request: {}", req),
//         }
//     }
// }

// >>>>>>> upstream/master
// /// Error type returned by the test reactor.
// #[derive(Debug, Error)]
// enum Error {
//     #[error("prometheus (metrics) error: {0}")]
//     Metrics(#[from] prometheus::Error),
// }

// impl Drop for Reactor {
//     fn drop(&mut self) {
//         NetworkController::<Message>::remove_node(&self.network.node_id())
//     }
// }

// <<<<<<< HEAD
// #[derive(Debug)]
// pub struct FetcherTestConfig {
//     gossiper_config: GossipConfig,
//     storage_config: storage::Config,
//     temp_dir: TempDir,
// }

// impl Default for FetcherTestConfig {
//     fn default() -> Self {
//         let (storage_config, temp_dir) = storage::Config::default_for_tests();
//         FetcherTestConfig {
//             gossiper_config: Default::default(),
//             storage_config,
//             temp_dir,
//         }
//     }
// }

// reactor!(Reactor {
//     type Config = FetcherTestConfig;
// =======
// impl reactor::Reactor for Reactor {
//     type Event = Event;
//     type Config = Config;
//     type Error = Error;

//     fn new(
//         config: Self::Config,
//         _registry: &Registry,
//         event_queue: EventQueueHandle<Self::Event>,
//         rng: &mut NodeRng,
//     ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
//         let network = NetworkController::create_node(event_queue, rng);

//         let (storage_config, _storage_tempdir) = storage::Config::default_for_tests();
//         let storage = Storage::new(&WithDir::new(_storage_tempdir.path(),
// storage_config)).unwrap(); >>>>>>> upstream/master

//     components: {
//         network = infallible InMemoryNetwork::<Message>(event_queue, rng);
//         storage = Storage(WithDir::new(cfg.temp_dir.path(), cfg.storage_config));
//         deploy_acceptor = infallible DeployAcceptor();
//         deploy_fetcher = infallible Fetcher::<Deploy>(cfg.gossiper_config);
//     }

//     events: {
//         network = Event<Message>;
//         storage = Event<Storage>;
//         deploy_fetcher = Event<Deploy>;
//     }

//     requests: {
//         // This tests contains no linear chain requests, so we panic if we receive any.
//         LinearChainRequest<NodeId> -> !;
//         NetworkRequest<NodeId, Message> -> network;
//         StorageRequest<Storage> -> storage;
//         FetcherRequest<NodeId, Deploy> -> deploy_fetcher;
//     }

//     announcements: {
//         // The deploy fetcher needs to be notified about new deploys.
//         DeployAcceptorAnnouncement<NodeId> -> [deploy_fetcher];
//         // TODO: Route network messages
//         NetworkAnnouncement<NodeId, Message> -> [fn handle_message];
//         // Currently the ApiServerAnnouncement is misnamed - it solely tells of new deploys
// arriving         // from a client.
//         ApiServerAnnouncement -> [deploy_acceptor];
//     }
// });

// impl Reactor {
//     fn handle_message(
//         &mut self,
// <<<<<<< HEAD
//         effect_builder: EffectBuilder<ReactorEvent>,
//         rng: &mut dyn CryptoRngCore,
//         network_announcement: NetworkAnnouncement<NodeId, Message>,
//     ) -> Effects<ReactorEvent> {
//         // TODO: Make this manual routing disappear and supply appropriate
//         // announcements.
//         match network_announcement {
//             NetworkAnnouncement::MessageReceived { sender, payload } => match payload {
//                 Message::GetRequest { serialized_id, .. } => {
//                     let deploy_hash = match bincode::deserialize(&serialized_id) {
//                         Ok(hash) => hash,
//                         Err(error) => {
//                             error!(
//                                 "failed to decode {:?} from {}: {}",
//                                 serialized_id, sender, error
//                             );
//                             return Effects::new();
//                         }
//                     };

//                     self.dispatch_event(
//                         effect_builder,
//                         rng,
//                         ReactorEvent::Storage(storage::Event::GetDeployForPeer {
//                             deploy_hash,
//                             peer: sender,
//                         }),
//                     )
//                 }

//                 Message::GetResponse {
//                     serialized_item, ..
//                 } => {
//                     let deploy = match bincode::deserialize(&serialized_item) {
//                         Ok(deploy) => Box::new(deploy),
//                         Err(error) => {
//                             error!("failed to decode deploy from {}: {}", sender, error);
//                             return Effects::new();
//                         }
//                     };

//                     self.dispatch_event(
//                         effect_builder,
//                         rng,
//                         ReactorEvent::DeployAcceptor(deploy_acceptor::Event::Accept {
//                             deploy,
//                             source: Source::Peer(sender),
//                         }),
//                     )
//                 }
//                 msg => panic!("should not get {}", msg),
//             },
//             ann => panic!("should not received any network announcements: {:?}", ann),
// =======
//         effect_builder: EffectBuilder<Self::Event>,
//         rng: &mut NodeRng,
//         event: Event,
//     ) -> Effects<Self::Event> {
//         match event {
//             Event::Storage(storage::Event::StorageRequest(StorageRequest::GetChainspec {
//                 responder,
//                 ..
//             })) => responder
//                 .respond(Some(Arc::new(Chainspec::from_resources(
//                     "local/chainspec.toml",
//                 ))))
//                 .ignore(),
//             Event::Storage(event) => reactor::wrap_effects(
//                 Event::Storage,
//                 self.storage.handle_event(effect_builder, rng, event),
//             ),
//             Event::DeployAcceptor(event) => reactor::wrap_effects(
//                 Event::DeployAcceptor,
//                 self.deploy_acceptor
//                     .handle_event(effect_builder, rng, event),
//             ),
//             Event::DeployFetcher(deploy_event) => reactor::wrap_effects(
//                 Event::DeployFetcher,
//                 self.deploy_fetcher
//                     .handle_event(effect_builder, rng, deploy_event),
//             ),
//             Event::NetworkRequest(request) => reactor::wrap_effects(
//                 Event::NetworkRequest,
//                 self.network.handle_event(effect_builder, rng, request),
//             ),
//             Event::DeployFetcherRequest(request) => reactor::wrap_effects(
//                 Event::DeployFetcher,
//                 self.deploy_fetcher
//                     .handle_event(effect_builder, rng, request.into()),
//             ),
//             Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
//                 sender,
//                 payload,
//             }) => {
//                 let reactor_event = match payload {
//                     Message::GetRequest {
//                         tag: Tag::Deploy,
//                         serialized_id,
//                     } => {
//                         // Note: This is copied almost verbatim from the validator reactor and
// needs                         // to be refactored.

//                         let deploy_hash = match bincode::deserialize(&serialized_id) {
//                             Ok(hash) => hash,
//                             Err(error) => {
//                                 error!(
//                                     "failed to decode {:?} from {}: {}",
//                                     serialized_id, sender, error
//                                 );
//                                 return Effects::new();
//                             }
//                         };

//                         match self
//                             .storage
//                             .handle_legacy_direct_deploy_request(deploy_hash)
//                         {
//                             // This functionality was moved out of the storage component and
//                             // should be refactored ASAP.
//                             Some(deploy) => {
//                                 match Message::new_get_response(&deploy) {
//                                     Ok(message) => {
//                                         return effect_builder
//                                             .send_message(sender, message)
//                                             .ignore();
//                                     }
//                                     Err(error) => {
//                                         error!("failed to create get-response: {}", error);
//                                         return Effects::new();
//                                     }
//                                 };
//                             }
//                             None => {
//                                 debug!("failed to get {} for {}", deploy_hash, sender);
//                                 return Effects::new();
//                             }
//                         }
//                     }
//                     Message::GetResponse {
//                         tag: Tag::Deploy,
//                         serialized_item,
//                     } => {
//                         let deploy = match bincode::deserialize(&serialized_item) {
//                             Ok(deploy) => Box::new(deploy),
//                             Err(error) => {
//                                 error!("failed to decode deploy from {}: {}", sender, error);
//                                 return Effects::new();
//                             }
//                         };
//                         Event::DeployAcceptor(deploy_acceptor::Event::Accept {
//                             deploy,
//                             source: Source::Peer(sender),
//                         })
//                     }
//                     msg => panic!("should not get {}", msg),
//                 };
//                 self.dispatch_event(effect_builder, rng, reactor_event)
//             }
//             Event::NetworkAnnouncement(ann) => {
//                 unreachable!("should not receive announcements of type {:?}", ann);
//             }
//             Event::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived { deploy }) => {
//                 let event = deploy_acceptor::Event::Accept {
//                     deploy,
//                     source: Source::<NodeId>::Client,
//                 };
//                 self.dispatch_event(effect_builder, rng, Event::DeployAcceptor(event))
//             }
//             Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::AcceptedNewDeploy {
//                 deploy,
//                 source,
//             }) => {
//                 let event = super::Event::GotRemotely {
//                     item: deploy,
//                     source,
//                 };
//                 self.dispatch_event(effect_builder, rng, Event::DeployFetcher(event))
//             }
//             Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
//                 deploy: _,
//                 source: _,
//             }) => Effects::new(),
//             Event::LinearChainRequest(_) => panic!("No linear chain requests in the test."),
// >>>>>>> upstream/master
//         }
//     }
// }

// impl NetworkedReactor for Reactor {
//     type NodeId = NodeId;

//     fn node_id(&self) -> NodeId {
//         self.network.node_id()
//     }
// }

// fn announce_deploy_received(
//     deploy: Deploy,
// ) -> impl FnOnce(EffectBuilder<ReactorEvent>) -> Effects<ReactorEvent> {
//     |effect_builder: EffectBuilder<ReactorEvent>| {
//         effect_builder
//             .announce_deploy_received(Box::new(deploy))
//             .ignore()
//     }
// }

// fn fetch_deploy(
//     deploy_hash: DeployHash,
//     node_id: NodeId,
//     fetched: Arc<Mutex<(bool, Option<FetchResult<Deploy>>)>>,
// ) -> impl FnOnce(EffectBuilder<ReactorEvent>) -> Effects<ReactorEvent> {
//     move |effect_builder: EffectBuilder<ReactorEvent>| {
//         effect_builder
//             .fetch_deploy(deploy_hash, node_id)
//             .then(move |maybe_deploy| async move {
//                 let mut result = fetched.lock().unwrap();
//                 result.0 = true;
//                 result.1 = maybe_deploy;
//             })
//             .ignore()
//     }
// }

// /// Store a deploy on a target node.
// async fn store_deploy(
//     deploy: &Deploy,
//     node_id: &NodeId,
//     network: &mut Network<Reactor>,
//     mut rng: &mut TestRng,
// ) {
//     network
//         .process_injected_effect_on(node_id, announce_deploy_received(deploy.clone()))
//         .await;

//     // cycle to deploy acceptor announcement
//     network
//         .crank_until(
//             node_id,
//             &mut rng,
// <<<<<<< HEAD
//             move |event: &ReactorEvent| -> bool {
//                 match event {
//                     ReactorEvent::DeployAcceptorAnnouncement(
// =======
//             move |event: &Event| -> bool {
//                 matches!(
//                     event,
//                     Event::DeployAcceptorAnnouncement(
// >>>>>>> upstream/master
//                         DeployAcceptorAnnouncement::AcceptedNewDeploy { .. },
//                     )
//                 )
//             },
//             TIMEOUT,
//         )
//         .await;
// }

// async fn assert_settled(
//     node_id: &NodeId,
//     deploy_hash: DeployHash,
//     expected_result: Option<FetchResult<Deploy>>,
//     fetched: Arc<Mutex<(bool, Option<FetchResult<Deploy>>)>>,
//     network: &mut Network<Reactor>,
//     rng: &mut TestRng,
//     timeout: Duration,
// ) {
//     let has_responded = |_nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
//         fetched.lock().unwrap().0
//     };

//     network.settle_on(rng, has_responded, timeout).await;

//     let maybe_stored_deploy = network
//         .nodes()
//         .get(node_id)
//         .unwrap()
//         .reactor()
//         .inner()
//         .storage
//         .get_deploy_by_hash(deploy_hash);
//     assert_eq!(expected_result.is_some(), maybe_stored_deploy.is_some());

//     assert_eq!(fetched.lock().unwrap().1, expected_result)
// }

// #[tokio::test]
// async fn should_fetch_from_local() {
//     const NETWORK_SIZE: usize = 1;

//     NetworkController::<Message>::create_active();
//     let (mut network, mut rng, node_ids) = {
//         let mut network = Network::<Reactor>::new();
//         let mut rng = crate::new_rng();
//         let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
//         (network, rng, node_ids)
//     };

//     // Create a random deploy.
//     let deploy = Deploy::random(&mut rng);

//     // Store deploy on a node.
//     let node_to_store_on = &node_ids[0];
//     store_deploy(&deploy, node_to_store_on, &mut network, &mut rng).await;

//     // Try to fetch the deploy from a node that holds it.
//     let node_id = &node_ids[0];
//     let deploy_hash = *deploy.id();
//     let fetched = Arc::new(Mutex::new((false, None)));
//     network
//         .process_injected_effect_on(
//             node_id,
//             fetch_deploy(deploy_hash, node_id.clone(), Arc::clone(&fetched)),
//         )
//         .await;

//     let expected_result = Some(FetchResult::FromStorage(Box::new(deploy)));
//     assert_settled(
//         node_id,
//         deploy_hash,
//         expected_result,
//         fetched,
//         &mut network,
//         &mut rng,
//         TIMEOUT,
//     )
//     .await;

//     NetworkController::<Message>::remove_active();
// }

// #[tokio::test]
// async fn should_fetch_from_peer() {
//     const NETWORK_SIZE: usize = 2;

//     NetworkController::<Message>::create_active();
//     let (mut network, mut rng, node_ids) = {
//         let mut network = Network::<Reactor>::new();
//         let mut rng = crate::new_rng();
//         let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
//         (network, rng, node_ids)
//     };

//     // Create a random deploy.
//     let deploy = Deploy::random(&mut rng);

//     // Store deploy on a node.
//     let node_with_deploy = &node_ids[0];
//     store_deploy(&deploy, node_with_deploy, &mut network, &mut rng).await;

//     let node_without_deploy = &node_ids[1];
//     let deploy_hash = *deploy.id();
//     let fetched = Arc::new(Mutex::new((false, None)));

//     // Try to fetch the deploy from a node that does not hold it; should get from peer.
//     network
//         .process_injected_effect_on(
//             node_without_deploy,
//             fetch_deploy(deploy_hash, node_with_deploy.clone(), Arc::clone(&fetched)),
//         )
//         .await;

//     let expected_result = Some(FetchResult::FromPeer(
//         Box::new(deploy),
//         node_with_deploy.clone(),
//     ));
//     assert_settled(
//         node_without_deploy,
//         deploy_hash,
//         expected_result,
//         fetched,
//         &mut network,
//         &mut rng,
//         TIMEOUT,
//     )
//     .await;

//     NetworkController::<Message>::remove_active();
// }

// #[tokio::test]
// async fn should_timeout_fetch_from_peer() {
//     const NETWORK_SIZE: usize = 2;

//     NetworkController::<Message>::create_active();
//     let (mut network, mut rng, node_ids) = {
//         let mut network = Network::<Reactor>::new();
//         let mut rng = crate::new_rng();
//         let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
//         (network, rng, node_ids)
//     };

//     // Create a random deploy.
//     let deploy = Deploy::random(&mut rng);
//     let deploy_hash = *deploy.id();

//     let holding_node = node_ids[0].clone();
//     let requesting_node = node_ids[1].clone();

//     // Store deploy on holding node.
//     store_deploy(&deploy, &holding_node, &mut network, &mut rng).await;

//     // Initiate requesting node asking for deploy from holding node.
//     let fetched = Arc::new(Mutex::new((false, None)));
//     network
//         .process_injected_effect_on(
//             &requesting_node,
//             fetch_deploy(deploy_hash, holding_node.clone(), Arc::clone(&fetched)),
//         )
//         .await;

//     // Crank until message sent from the requester.
//     network
//         .crank_until(
//             &requesting_node,
//             &mut rng,
// <<<<<<< HEAD
//             move |event: &ReactorEvent| -> bool {
//                 if let ReactorEvent::NetworkRequest(NetworkRequest::SendMessage {
//                     payload: Message::GetRequest { .. },
//                     ..
//                 }) = event
//                 {
//                     true
//                 } else {
//                     false
//                 }
// =======
//             move |event: &Event| -> bool {
//                 matches!(
//                     event,
//                     Event::NetworkRequest(NetworkRequest::SendMessage {
//                         payload: Message::GetRequest { .. },
//                         ..
//                     })
//                 )
// >>>>>>> upstream/master
//             },
//             TIMEOUT,
//         )
//         .await;

//     // Crank until the message is received by the holding node.
//     network
//         .crank_until(
//             &holding_node,
//             &mut rng,
// <<<<<<< HEAD
//             move |event: &ReactorEvent| -> bool {
//                 if let ReactorEvent::NetworkRequest(NetworkRequest::SendMessage {
//                     payload: Message::GetResponse { .. },
//                     ..
//                 }) = event
//                 {
//                     true
//                 } else {
//                     false
//                 }
// =======
//             move |event: &Event| -> bool {
//                 matches!(
//                     event,
//                     Event::NetworkRequest(NetworkRequest::SendMessage {
//                         payload: Message::GetResponse { .. },
//                         ..
//                     })
//                 )
// >>>>>>> upstream/master
//             },
//             TIMEOUT,
//         )
//         .await;

//     // Advance time.
//     let secs_to_advance = FetcherConfig::default().get_from_peer_timeout();
//     time::pause();
//     time::advance(Duration::from_secs(secs_to_advance + 10)).await;
//     time::resume();

//     // Settle the network, allowing timeout to avoid panic.
//     let expected_result = None;
//     assert_settled(
//         &requesting_node,
//         deploy_hash,
//         expected_result,
//         fetched,
//         &mut network,
//         &mut rng,
//         TIMEOUT,
//     )
//     .await;

//     NetworkController::<Message>::remove_active();
// }
