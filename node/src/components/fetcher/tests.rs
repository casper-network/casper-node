#![cfg(test)]

use std::{
    fmt::{self, Display, Formatter},
    sync::{Arc, Mutex},
};

use derive_more::From;
use futures::FutureExt;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;

use casper_node_macros::reactor;
use casper_types::testing::TestRng;

use super::*;
use crate::{
    components::{
        deploy_acceptor, fetcher,
        in_memory_network::{self, InMemoryNetwork, NetworkController},
        small_network::GossipedAddress,
        storage::{self, Storage},
    },
    effect::{
        announcements::{ControlAnnouncement, DeployAcceptorAnnouncement, RpcServerAnnouncement},
        incoming::{
            BlockAddedRequestIncoming, BlockAddedResponseIncoming, ConsensusMessageIncoming,
            FinalitySignatureIncoming, GossiperIncoming, NetRequestIncoming, NetResponse,
            NetResponseIncoming, SyncLeapRequestIncoming, SyncLeapResponseIncoming, TrieDemand,
            TrieRequestIncoming, TrieResponseIncoming,
        },
        requests::{MarkBlockCompletedRequest, StateStoreRequest},
        Responder,
    },
    fatal,
    protocol::Message,
    reactor::{self, EventQueueHandle, Reactor as ReactorTrait, ReactorEvent, ReactorExit, Runner},
    testing::{
        self,
        network::{Network, NetworkedReactor},
        ConditionCheckReactor, FakeDeployAcceptor,
    },
    types::{
        BlockAdded, Chainspec, ChainspecRawBytes, Deploy, DeployHash, FinalitySignature,
        GossiperItem, NodeId,
    },
    utils::WithDir,
};

const TIMEOUT: Duration = Duration::from_secs(1);

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
enum Error {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

impl Drop for Reactor {
    fn drop(&mut self) {
        NetworkController::<Message>::remove_node(&self.network.node_id())
    }
}

#[derive(Debug)]
pub struct FetcherTestConfig {
    fetcher_config: Config,
    storage_config: storage::Config,
    temp_dir: TempDir,
}

impl Default for FetcherTestConfig {
    fn default() -> Self {
        let (storage_config, temp_dir) = storage::Config::default_for_tests();
        FetcherTestConfig {
            fetcher_config: Default::default(),
            storage_config,
            temp_dir,
        }
    }
}

#[derive(Debug, From, Serialize)]
enum Event {
    #[from]
    Network(in_memory_network::Event<Message>),
    #[from]
    Storage(storage::Event),
    #[from]
    FakeDeployAcceptor(deploy_acceptor::Event),
    #[from]
    DeployFetcher(fetcher::Event<Deploy>),
    #[from]
    NetworkRequestMessage(NetworkRequest<Message>),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    StateStoreRequest(StateStoreRequest),
    #[from]
    FetcherRequestDeploy(FetcherRequest<Deploy>),
    #[from]
    DeployAcceptorAnnouncement(DeployAcceptorAnnouncement),
    #[from]
    RpcServerAnnouncement(RpcServerAnnouncement),
    #[from]
    NetRequestIncoming(NetRequestIncoming),
    #[from]
    NetResponseIncoming(NetResponseIncoming),
    #[from]
    BlocklistAnnouncement(BlocklistAnnouncement),
    #[from]
    MarkBlockCompletedRequest(MarkBlockCompletedRequest),
    #[from]
    TrieDemand(TrieDemand),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
    #[from]
    GossiperIncomingDeploy(GossiperIncoming<Deploy>),
    #[from]
    GossiperIncomingBlockAdded(GossiperIncoming<BlockAdded>),
    #[from]
    GossiperIncomingFinalitySignature(GossiperIncoming<FinalitySignature>),
    #[from]
    GossiperIncomingGossipedAddress(GossiperIncoming<GossipedAddress>),
    #[from]
    TrieRequestIncoming(TrieRequestIncoming),
    #[from]
    TrieResponseIncoming(TrieResponseIncoming),
    #[from]
    SyncLeapRequestIncoming(SyncLeapRequestIncoming),
    #[from]
    SyncLeapResponseIncoming(SyncLeapResponseIncoming),
    #[from]
    BlockAddedRequestIncoming(BlockAddedRequestIncoming),
    #[from]
    BlockAddedResponseIncoming(BlockAddedResponseIncoming),
    #[from]
    ConsensusMessageIncoming(ConsensusMessageIncoming),
    #[from]
    FinalitySignatureIncoming(FinalitySignatureIncoming),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

impl ReactorEvent for Event {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        None
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        None
    }
}

#[derive(Debug)]
struct Reactor {
    network: InMemoryNetwork<Message>,
    storage: Storage,
    fake_deploy_acceptor: FakeDeployAcceptor,
    deploy_fetcher: Fetcher<Deploy>,
}

impl ReactorTrait for Reactor {
    type Event = Event;
    type Config = FetcherTestConfig;
    type Error = Error;

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network.handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::FakeDeployAcceptor(event) => reactor::wrap_effects(
                Event::FakeDeployAcceptor,
                self.fake_deploy_acceptor
                    .handle_event(effect_builder, rng, event.into()),
            ),
            Event::DeployFetcher(event) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher
                    .handle_event(effect_builder, rng, event.into()),
            ),
            Event::NetworkRequestMessage(request) => reactor::wrap_effects(
                Event::Network,
                self.network
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::StorageRequest(request) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::StateStoreRequest(request) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::FetcherRequestDeploy(request) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::DeployAcceptorAnnouncement(announcement) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher
                    .handle_event(effect_builder, rng, announcement.into()),
            ),
            Event::RpcServerAnnouncement(announcement) => reactor::wrap_effects(
                Event::FakeDeployAcceptor,
                self.fake_deploy_acceptor
                    .handle_event(effect_builder, rng, announcement.into()),
            ),
            Event::NetRequestIncoming(announcement) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, announcement.into()),
            ),
            Event::NetResponseIncoming(announcement) => {
                let mut announcement_effects = Effects::new();
                let effects = self.handle_net_response(effect_builder, rng, announcement);
                announcement_effects.extend(effects.into_iter());
                announcement_effects
            }
            Event::MarkBlockCompletedRequest(request) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::TrieDemand(_)
            | Event::ContractRuntimeRequest(_)
            | Event::BlocklistAnnouncement(_)
            | Event::GossiperIncomingDeploy(_)
            | Event::GossiperIncomingBlockAdded(_)
            | Event::GossiperIncomingFinalitySignature(_)
            | Event::GossiperIncomingGossipedAddress(_)
            | Event::TrieRequestIncoming(_)
            | Event::TrieResponseIncoming(_)
            | Event::SyncLeapRequestIncoming(_)
            | Event::SyncLeapResponseIncoming(_)
            | Event::BlockAddedRequestIncoming(_)
            | Event::BlockAddedResponseIncoming(_)
            | Event::ConsensusMessageIncoming(_)
            | Event::FinalitySignatureIncoming(_)
            | Event::ControlAnnouncement(_) => panic!("unexpected: {}", event),
        }
    }

    fn new(
        cfg: Self::Config,
        chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let network = InMemoryNetwork::<Message>::new(event_queue, rng);

        let storage = Storage::new(
            &WithDir::new(cfg.temp_dir.path(), cfg.storage_config),
            chainspec.highway_config.finality_threshold_fraction,
            chainspec.hard_reset_to_start_of_era(),
            chainspec.protocol_config.version,
            &chainspec.network_config.name,
            chainspec.core_config.unbonding_delay,
        )
        .unwrap();

        let fake_deploy_acceptor = FakeDeployAcceptor::new();
        let deploy_fetcher =
            Fetcher::<Deploy>::new("deploy", &cfg.fetcher_config, registry).unwrap();
        let reactor = Reactor {
            network,
            storage,
            fake_deploy_acceptor,
            deploy_fetcher,
        };
        Ok((reactor, Effects::new()))
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        None
    }
}

impl Reactor {
    fn handle_net_response(
        &mut self,
        effect_builder: EffectBuilder<Event>,
        rng: &mut NodeRng,
        response: NetResponseIncoming,
    ) -> Effects<Event> {
        match response.message {
            NetResponse::Deploy(ref serialized_item) => {
                let deploy = match bincode::deserialize::<FetchResponse<Deploy, DeployHash>>(
                    serialized_item,
                ) {
                    Ok(FetchResponse::Fetched(deploy)) => Box::new(deploy),
                    Ok(FetchResponse::NotFound(deploy_hash)) => {
                        return fatal!(
                            effect_builder,
                            "peer did not have deploy with hash {}: {}",
                            deploy_hash,
                            response.sender,
                        )
                        .ignore();
                    }
                    Ok(FetchResponse::NotProvided(deploy_hash)) => {
                        return fatal!(
                            effect_builder,
                            "peer refused to provide deploy with hash {}: {}",
                            deploy_hash,
                            response.sender,
                        )
                        .ignore();
                    }
                    Err(error) => {
                        return fatal!(
                            effect_builder,
                            "failed to decode deploy from {}: {}",
                            response.sender,
                            error
                        )
                        .ignore();
                    }
                };

                self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::FakeDeployAcceptor(deploy_acceptor::Event::Accept {
                        deploy,
                        source: Source::Peer(response.sender),
                        maybe_responder: None,
                    }),
                )
            }
            _ => fatal!(
                effect_builder,
                "no support for anything but deploy responses in fetcher test"
            )
            .ignore(),
        }
    }
}

impl NetworkedReactor for Reactor {
    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn announce_deploy_received(
    deploy: Deploy,
    responder: Option<Responder<Result<(), deploy_acceptor::Error>>>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .announce_deploy_received(Box::new(deploy), responder)
            .ignore()
    }
}

type FetchedDeployResult = Arc<Mutex<(bool, Option<FetchResult<Deploy>>)>>;

fn fetch_deploy(
    deploy_hash: DeployHash,
    node_id: NodeId,
    fetched: FetchedDeployResult,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    move |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .fetch::<Deploy>(deploy_hash, node_id, ())
            .then(move |deploy| async move {
                let mut result = fetched.lock().unwrap();
                result.0 = true;
                result.1 = Some(deploy);
            })
            .ignore()
    }
}

/// Store a deploy on a target node.
async fn store_deploy(
    deploy: &Deploy,
    node_id: &NodeId,
    network: &mut Network<Reactor>,
    responder: Option<Responder<Result<(), deploy_acceptor::Error>>>,
    rng: &mut TestRng,
) {
    network
        .process_injected_effect_on(node_id, announce_deploy_received(deploy.clone(), responder))
        .await;

    // cycle to deploy acceptor announcement
    network
        .crank_until(
            node_id,
            rng,
            move |event: &Event| {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy { .. },
                    )
                )
            },
            TIMEOUT,
        )
        .await;
}

#[derive(Debug)]
enum ExpectedFetchedDeployResult {
    TimedOut,
    FromStorage {
        expected_deploy: Box<Deploy>,
    },
    FromPeer {
        expected_deploy: Box<Deploy>,
        expected_peer: NodeId,
    },
}

async fn assert_settled(
    node_id: &NodeId,
    deploy_hash: DeployHash,
    expected_result: ExpectedFetchedDeployResult,
    fetched: FetchedDeployResult,
    network: &mut Network<Reactor>,
    rng: &mut TestRng,
    timeout: Duration,
) {
    let has_responded = |_nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
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
        .get_deploy_by_hash(deploy_hash);

    // assert_eq!(expected_result.is_some(), maybe_stored_deploy.is_some());
    let actual_fetcher_result = fetched.lock().unwrap().1.clone();
    match (expected_result, actual_fetcher_result, maybe_stored_deploy) {
        // Timed-out case: despite the delayed response causing a timeout, the response does arrive,
        // and the TestDeployAcceptor unconditionally accepts the deploy and stores it. For the
        // test, we don't care whether it was stored or not, just that the TimedOut event fired.
        (ExpectedFetchedDeployResult::TimedOut, Some(Err(fetcher::Error::TimedOut { .. })), _) => {}
        // FromStorage case: expect deploy to correspond to item fetched, as well as stored item
        (
            ExpectedFetchedDeployResult::FromStorage { expected_deploy },
            Some(Ok(FetchedData::FromStorage { item })),
            Some(stored_deploy),
        ) if expected_deploy == item && stored_deploy == *item => {}
        // FromPeer case: deploys should correspond, storage should be present and correspond, and
        // peers should correspond.
        (
            ExpectedFetchedDeployResult::FromPeer {
                expected_deploy,
                expected_peer,
            },
            Some(Ok(FetchedData::FromPeer { item, peer })),
            Some(stored_deploy),
        ) if expected_deploy == item && stored_deploy == *item && expected_peer == peer => {}
        // Sad path case
        (expected_result, actual_fetcher_result, maybe_stored_deploy) => {
            panic!(
                "Expected result type {:?} but found {:?} (stored deploy is {:?})",
                expected_result, actual_fetcher_result, maybe_stored_deploy
            )
        }
    }
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
    let deploy = Deploy::random_valid_native_transfer(&mut rng);

    // Store deploy on a node.
    let node_to_store_on = &node_ids[0];
    store_deploy(&deploy, node_to_store_on, &mut network, None, &mut rng).await;

    // Try to fetch the deploy from a node that holds it.
    let node_id = node_ids[0];
    let deploy_hash = *deploy.id();
    let fetched = Arc::new(Mutex::new((false, None)));
    network
        .process_injected_effect_on(
            &node_id,
            fetch_deploy(deploy_hash, node_id, Arc::clone(&fetched)),
        )
        .await;

    let expected_result = ExpectedFetchedDeployResult::FromStorage {
        expected_deploy: Box::new(deploy),
    };
    assert_settled(
        &node_id,
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
    let deploy = Deploy::random_valid_native_transfer(&mut rng);

    // Store deploy on a node.
    let node_with_deploy = node_ids[0];
    store_deploy(&deploy, &node_with_deploy, &mut network, None, &mut rng).await;

    let node_without_deploy = node_ids[1];
    let deploy_hash = *deploy.id();
    let fetched = Arc::new(Mutex::new((false, None)));

    // Try to fetch the deploy from a node that does not hold it; should get from peer.
    network
        .process_injected_effect_on(
            &node_without_deploy,
            fetch_deploy(deploy_hash, node_with_deploy, Arc::clone(&fetched)),
        )
        .await;

    let expected_result = ExpectedFetchedDeployResult::FromPeer {
        expected_deploy: Box::new(deploy),
        expected_peer: node_with_deploy,
    };
    assert_settled(
        &node_without_deploy,
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
    let deploy = Deploy::random_valid_native_transfer(&mut rng);
    let deploy_hash = *deploy.id();

    let holding_node = node_ids[0];
    let requesting_node = node_ids[1];

    // Store deploy on holding node.
    store_deploy(&deploy, &holding_node, &mut network, None, &mut rng).await;

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
            move |event: &Event| {
                if let Event::NetworkRequestMessage(NetworkRequest::SendMessage {
                    payload, ..
                }) = event
                {
                    matches!(**payload, Message::GetRequest { .. })
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
            move |event: &Event| {
                if let Event::NetworkRequestMessage(NetworkRequest::SendMessage {
                    payload, ..
                }) = event
                {
                    matches!(**payload, Message::GetResponse { .. })
                } else {
                    false
                }
            },
            TIMEOUT,
        )
        .await;

    // Advance time.
    let duration_to_advance: Duration = Config::default().get_from_peer_timeout().into();
    let duration_to_advance = duration_to_advance + Duration::from_secs(10);
    testing::advance_time(duration_to_advance).await;

    // Settle the network, allowing timeout to avoid panic.
    let expected_result = ExpectedFetchedDeployResult::TimedOut;
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
