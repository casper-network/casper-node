#![cfg(test)]
#![allow(unreachable_code)]

use std::sync::{Arc, Mutex};

use casper_node_macros::reactor;
use futures::FutureExt;
use tempfile::TempDir;
use thiserror::Error;

use super::*;
use crate::{
    components::{deploy_acceptor, in_memory_network::NetworkController, storage},
    effect::{
        announcements::{DeployAcceptorAnnouncement, NetworkAnnouncement},
        Responder,
    },
    fatal,
    protocol::Message,
    reactor::{Reactor as ReactorTrait, Runner},
    testing,
    testing::{
        network::{Network, NetworkedReactor},
        ConditionCheckReactor, TestRng,
    },
    types::{Deploy, DeployHash, NodeId},
    utils::{WithDir, RESOURCES_PATH},
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
    deploy_acceptor_config: deploy_acceptor::Config,
    temp_dir: TempDir,
}

impl Default for FetcherTestConfig {
    fn default() -> Self {
        let (storage_config, temp_dir) = storage::Config::default_for_tests();
        FetcherTestConfig {
            fetcher_config: Default::default(),
            storage_config,
            deploy_acceptor_config: deploy_acceptor::Config::new(false),
            temp_dir,
        }
    }
}

reactor!(Reactor {
    type Config = FetcherTestConfig;

    components: {
        chainspec_loader = has_effects ChainspecLoader(
            &RESOURCES_PATH.join("local"),
            effect_builder
        );
        network = infallible InMemoryNetwork::<Message>(event_queue, rng);
        storage = Storage(
            &WithDir::new(cfg.temp_dir.path(), cfg.storage_config),
            chainspec_loader.hard_reset_to_start_of_era(),
            chainspec_loader.chainspec().protocol_config.version,
            false,
            &chainspec_loader.chainspec().network_config.name,
            chainspec_loader
                .chainspec()
                .highway_config
                .finality_threshold_fraction,
            chainspec_loader
                .chainspec()
                .protocol_config
                .last_emergency_restart,
        );
        deploy_acceptor = DeployAcceptor(cfg.deploy_acceptor_config, &*chainspec_loader.chainspec(), registry);
        deploy_fetcher = Fetcher::<Deploy>("deploy", cfg.fetcher_config, registry);
    }

    events: {
        network = Event<Message>;
        deploy_fetcher = Event<Deploy>;
    }

    requests: {
        // This test contains no linear chain requests, so we panic if we receive any.
        NetworkRequest<NodeId, Message> -> network;
        StorageRequest -> storage;
        StateStoreRequest -> storage;
        FetcherRequest<NodeId, Deploy> -> deploy_fetcher;

        // The only contract runtime request will be the commit of genesis, which we discard.
        ContractRuntimeRequest -> #;
    }

    announcements: {
        // The deploy fetcher needs to be notified about new deploys.
        DeployAcceptorAnnouncement<NodeId> -> [deploy_fetcher];
        NetworkAnnouncement<NodeId, Message> -> [fn handle_message];
        // Currently the RpcServerAnnouncement is misnamed - it solely tells of new deploys arriving
        // from a client.
        RpcServerAnnouncement -> [deploy_acceptor];
        ChainspecLoaderAnnouncement -> [!];
        BlocklistAnnouncement<NodeId> -> [!];
    }
});

impl Reactor {
    fn handle_message(
        &mut self,
        effect_builder: EffectBuilder<ReactorEvent>,
        rng: &mut NodeRng,
        network_announcement: NetworkAnnouncement<NodeId, Message>,
    ) -> Effects<ReactorEvent> {
        // TODO: Make this manual routing disappear and supply appropriate
        // announcements.
        match network_announcement {
            NetworkAnnouncement::MessageReceived { sender, payload } => match payload {
                Message::GetRequest { serialized_id, .. } => {
                    let deploy_hash: DeployHash = match bincode::deserialize(&serialized_id) {
                        Ok(hash) => hash,
                        Err(error) => {
                            error!(
                                "failed to decode {:?} from {}: {}",
                                serialized_id, sender, error
                            );
                            return Effects::new();
                        }
                    };

                    let fetched_or_not_found_deploy = match self.storage.get_deploy(deploy_hash) {
                        Ok(Some(deploy)) => FetchedOrNotFound::Fetched(deploy),
                        Ok(None) | Err(_) => FetchedOrNotFound::NotFound(deploy_hash),
                    };

                    match Message::new_get_response(&fetched_or_not_found_deploy) {
                        Ok(message) => effect_builder.send_message(sender, message).ignore(),
                        Err(error) => {
                            error!("failed to create get-response: {}", error);
                            Effects::new()
                        }
                    }
                }

                Message::GetResponse {
                    serialized_item, ..
                } => {
                    let deploy = match bincode::deserialize::<FetchedOrNotFound<Deploy, DeployHash>>(
                        &serialized_item,
                    ) {
                        Ok(FetchedOrNotFound::Fetched(deploy)) => Box::new(deploy),
                        Ok(FetchedOrNotFound::NotFound(deploy_hash)) => {
                            return fatal!(
                                effect_builder,
                                "peer did not have deploy with hash {}: {}",
                                deploy_hash,
                                sender,
                            )
                            .ignore();
                        }
                        Err(error) => {
                            return fatal!(
                                effect_builder,
                                "failed to decode deploy from {}: {}",
                                sender,
                                error
                            )
                            .ignore();
                        }
                    };

                    self.dispatch_event(
                        effect_builder,
                        rng,
                        ReactorEvent::DeployAcceptor(deploy_acceptor::Event::Accept {
                            deploy,
                            source: Source::Peer(sender),
                            maybe_responder: None,
                        }),
                    )
                }
                msg => panic!("should not get {}", msg),
            },
            ann => panic!("should not received any network announcements: {:?}", ann),
        }
    }
}

impl NetworkedReactor for Reactor {
    type NodeId = NodeId;

    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn announce_deploy_received(
    deploy: Deploy,
    responder: Option<Responder<Result<(), deploy_acceptor::Error>>>,
) -> impl FnOnce(EffectBuilder<ReactorEvent>) -> Effects<ReactorEvent> {
    |effect_builder: EffectBuilder<ReactorEvent>| {
        effect_builder
            .announce_deploy_received(Box::new(deploy), responder)
            .ignore()
    }
}

type FetchedDeployResult = Arc<Mutex<(bool, Option<FetchResult<Deploy, NodeId>>)>>;

fn fetch_deploy(
    deploy_hash: DeployHash,
    node_id: NodeId,
    fetched: FetchedDeployResult,
) -> impl FnOnce(EffectBuilder<ReactorEvent>) -> Effects<ReactorEvent> {
    move |effect_builder: EffectBuilder<ReactorEvent>| {
        effect_builder
            .fetch::<Deploy, NodeId>(deploy_hash, node_id)
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
    mut rng: &mut TestRng,
) {
    network
        .process_injected_effect_on(node_id, announce_deploy_received(deploy.clone(), responder))
        .await;

    // cycle to deploy acceptor announcement
    network
        .crank_until(
            node_id,
            &mut rng,
            move |event: &ReactorEvent| {
                matches!(
                    event,
                    ReactorEvent::DeployAcceptorAnnouncement(
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
        // Timed-out case: should not have a stored deploy
        (ExpectedFetchedDeployResult::TimedOut, Some(Err(FetcherError::TimedOut { .. })), None) => {
        }
        // FromStorage case: expect deploy to correspond to item fetched, as well as stored item
        (
            ExpectedFetchedDeployResult::FromStorage { expected_deploy },
            Some(Ok(FetchedData::FromStorage { item })),
            Some(stored_deploy),
        ) if expected_deploy.equals_ignoring_is_valid(&*item)
            && stored_deploy.equals_ignoring_is_valid(&*item) => {}
        // FromPeer case: deploys should correspond, storage should be present and correspond, and
        // peers should correspond.
        (
            ExpectedFetchedDeployResult::FromPeer {
                expected_deploy,
                expected_peer,
            },
            Some(Ok(FetchedData::FromPeer { item, peer })),
            Some(stored_deploy),
        ) if expected_deploy.equals_ignoring_is_valid(&*item)
            && stored_deploy.equals_ignoring_is_valid(&*item)
            && expected_peer == peer => {}
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
            move |event: &ReactorEvent| {
                if let ReactorEvent::NetworkRequest(NetworkRequest::SendMessage {
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
            move |event: &ReactorEvent| {
                if let ReactorEvent::NetworkRequest(NetworkRequest::SendMessage {
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
