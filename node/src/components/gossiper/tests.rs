// Unrestricted event size is okay in tests.
#![allow(clippy::large_enum_variant)]
#![cfg(test)]
use std::{
    collections::{BTreeSet, HashMap},
    fmt::{self, Debug, Display, Formatter},
    iter,
    sync::Arc,
};

use derive_more::From;
use num_rational::Ratio;
use prometheus::Registry;
use rand::Rng;
use reactor::ReactorEvent;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;
use tokio::time;
use tracing::debug;

use casper_execution_engine::{
    core::engine_state::{
        engine_config::{
            DEFAULT_MINIMUM_DELEGATION_AMOUNT, DEFAULT_STRICT_ARGUMENT_CHECKING,
            DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS,
        },
        DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
    },
    shared::{system_config::SystemConfig, wasm_config::WasmConfig},
};
use casper_types::{testing::TestRng, ProtocolVersion};

use super::*;
use crate::{
    components::{
        contract_runtime::{self, ContractRuntime},
        deploy_acceptor,
        in_memory_network::{self, InMemoryNetwork, NetworkController},
        small_network::GossipedAddress,
        storage::{self, Storage},
    },
    effect::{
        announcements::{
            ContractRuntimeAnnouncement, ControlAnnouncement, DeployAcceptorAnnouncement,
            GossiperAnnouncement, RpcServerAnnouncement,
        },
        incoming::{
            BlockAddedRequestIncoming, BlockAddedResponseIncoming, ConsensusMessageIncoming,
            FinalitySignatureIncoming, NetRequestIncoming, NetResponse, NetResponseIncoming,
            SyncLeapRequestIncoming, SyncLeapResponseIncoming, TrieDemand, TrieRequestIncoming,
            TrieResponseIncoming,
        },
        requests::{ConsensusRequest, ContractRuntimeRequest, MarkBlockCompletedRequest},
        Responder,
    },
    fatal,
    protocol::Message as NodeMessage,
    reactor::{self, EventQueueHandle, Runner},
    testing::{
        self,
        network::{Network, NetworkedReactor},
        ConditionCheckReactor, FakeDeployAcceptor,
    },
    types::{Chainspec, ChainspecRawBytes, Deploy, FinalitySignature, NodeId},
    utils::WithDir,
    NodeRng,
};

const MAX_ASSOCIATED_KEYS: u32 = 100;
const RECENT_ERA_COUNT: u64 = 5;

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
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
    BlockAddedGossiper(super::Event<BlockAdded>),
    #[from]
    NetworkRequest(NetworkRequest<NodeMessage>),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    MarkBlockCompletedRequest(MarkBlockCompletedRequest),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement),
    #[from]
    DeployGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<Deploy>),
    #[from]
    BlockAddedGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<BlockAdded>),
    #[from]
    FinalitySignatureGossiperAnnouncement(
        #[serde(skip_serializing)] GossiperAnnouncement<FinalitySignature>,
    ),
    #[from]
    ContractRuntime(contract_runtime::Event),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
    #[from]
    ConsensusMessageIncoming(ConsensusMessageIncoming),
    #[from]
    DeployGossiperIncoming(GossiperIncoming<Deploy>),
    #[from]
    BlockAddedGossiperIncoming(GossiperIncoming<BlockAdded>),
    #[from]
    FinalitySignatureGossiperIncoming(GossiperIncoming<FinalitySignature>),
    #[from]
    AddressGossiperIncoming(GossiperIncoming<GossipedAddress>),
    #[from]
    NetRequestIncoming(NetRequestIncoming),
    #[from]
    NetResponseIncoming(NetResponseIncoming),
    #[from]
    TrieRequestIncoming(TrieRequestIncoming),
    #[from]
    TrieDemand(TrieDemand),
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
    FinalitySignatureIncoming(FinalitySignatureIncoming),
}

impl ReactorEvent for Event {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

impl From<NetworkRequest<Message<Deploy>>> for Event {
    fn from(request: NetworkRequest<Message<Deploy>>) -> Self {
        Event::NetworkRequest(request.map_payload(NodeMessage::from))
    }
}

impl From<NetworkRequest<Message<BlockAdded>>> for Event {
    fn from(request: NetworkRequest<Message<BlockAdded>>) -> Self {
        Event::NetworkRequest(request.map_payload(NodeMessage::from))
    }
}

impl From<ConsensusRequest> for Event {
    fn from(_request: ConsensusRequest) -> Self {
        unimplemented!("not implemented for gossiper tests")
    }
}

impl From<ContractRuntimeAnnouncement> for Event {
    fn from(_request: ContractRuntimeAnnouncement) -> Self {
        unimplemented!("not implemented for gossiper tests")
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(formatter, "event: {}", event),
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::DeployAcceptor(event) => write!(formatter, "deploy acceptor: {}", event),
            Event::DeployGossiper(event) => write!(formatter, "deploy gossiper: {}", event),
            Event::BlockAddedGossiper(event) => write!(formatter, "block gossiper: {}", event),
            Event::StorageRequest(req) => write!(formatter, "storage request: {}", req),
            Event::MarkBlockCompletedRequest(req) => {
                write!(formatter, "mark block completed: {}", req)
            }
            Event::NetworkRequest(req) => write!(formatter, "network request: {}", req),
            Event::ContractRuntimeRequest(req) => write!(formatter, "incoming: {}", req),
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::RpcServerAnnouncement(ann) => {
                write!(formatter, "api server announcement: {}", ann)
            }
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(formatter, "deploy-acceptor announcement: {}", ann)
            }
            Event::DeployGossiperAnnouncement(ann) => {
                write!(formatter, "deploy-gossiper announcement: {}", ann)
            }
            Event::BlockAddedGossiperAnnouncement(ann) => {
                write!(formatter, "block-gossiper announcement: {}", ann)
            }
            Event::FinalitySignatureGossiperAnnouncement(ann) => {
                write!(
                    formatter,
                    "finality-signature-gossiper announcement: {}",
                    ann
                )
            }
            Event::ContractRuntime(event) => {
                write!(formatter, "contract-runtime event: {:?}", event)
            }
            Event::ConsensusMessageIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::DeployGossiperIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::BlockAddedGossiperIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::FinalitySignatureGossiperIncoming(inner) => {
                write!(formatter, "incoming: {}", inner)
            }
            Event::AddressGossiperIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::NetRequestIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::NetResponseIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::TrieRequestIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::TrieDemand(inner) => write!(formatter, "demand: {}", inner),
            Event::TrieResponseIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::SyncLeapRequestIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::SyncLeapResponseIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::BlockAddedRequestIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::BlockAddedResponseIncoming(inner) => write!(formatter, "incoming: {}", inner),
            Event::FinalitySignatureIncoming(inner) => write!(formatter, "incoming: {}", inner),
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
    network: InMemoryNetwork<NodeMessage>,
    storage: Storage,
    fake_deploy_acceptor: FakeDeployAcceptor,
    deploy_gossiper: Gossiper<Deploy, Event>,
    block_added_gossiper: Gossiper<BlockAdded, Event>,
    contract_runtime: ContractRuntime,
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
            RECENT_ERA_COUNT,
        )
        .unwrap();

        let contract_runtime_config = contract_runtime::Config::default();
        let contract_runtime = ContractRuntime::new(
            ProtocolVersion::from_parts(1, 0, 0),
            storage.root_path(),
            &contract_runtime_config,
            WasmConfig::default(),
            SystemConfig::default(),
            MAX_ASSOCIATED_KEYS,
            DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
            DEFAULT_MINIMUM_DELEGATION_AMOUNT,
            DEFAULT_STRICT_ARGUMENT_CHECKING,
            DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS,
            registry,
        )
        .unwrap();

        let fake_deploy_acceptor = FakeDeployAcceptor::new();
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config,
            get_deploy_from_storage::<Deploy, Event>,
            registry,
        )?;
        let block_added_gossiper = Gossiper::new_for_partial_items(
            "block_added_gossiper",
            config,
            get_block_added_from_storage::<BlockAdded, Event>,
            registry,
        )?;

        let reactor = Reactor {
            network,
            storage,
            fake_deploy_acceptor,
            deploy_gossiper,
            block_added_gossiper,
            contract_runtime,
            _storage_tempdir: storage_tempdir,
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
                self.fake_deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::DeployGossiper(event) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::BlockAddedGossiper(event) => reactor::wrap_effects(
                Event::BlockAddedGossiper,
                self.block_added_gossiper
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
            Event::MarkBlockCompletedRequest(_) => {
                panic!("gossiper tests should never mark blocks completed")
            }
            Event::ControlAnnouncement(ctrl_ann) => {
                unreachable!("unhandled control announcement: {}", ctrl_ann)
            }
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
                    item_id: *deploy.id(),
                    source,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployGossiper(event))
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                deploy: _,
                source: _,
            }) => Effects::new(),
            Event::DeployGossiperAnnouncement(_ann) => Effects::new(),
            Event::BlockAddedGossiperAnnouncement(_ann) => Effects::new(),
            Event::FinalitySignatureGossiperAnnouncement(_ann) => Effects::new(),
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network.handle_event(effect_builder, rng, event),
            ),
            Event::ContractRuntimeRequest(req) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            Event::DeployGossiperIncoming(incoming) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            Event::BlockAddedGossiperIncoming(incoming) => reactor::wrap_effects(
                Event::BlockAddedGossiper,
                self.block_added_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            Event::NetRequestIncoming(incoming) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            Event::NetResponseIncoming(NetResponseIncoming { sender, message }) => match message {
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
                                sender,
                            )
                            .ignore();
                        }
                        Ok(FetchResponse::NotProvided(deploy_hash)) => {
                            return fatal!(
                                effect_builder,
                                "peer refused to provide deploy with hash {}: {}",
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
                    reactor::wrap_effects(
                        Event::DeployAcceptor,
                        self.fake_deploy_acceptor.handle_event(
                            effect_builder,
                            rng,
                            deploy_acceptor::Event::Accept {
                                deploy,
                                source: Source::Peer(sender),
                                maybe_responder: None,
                            },
                        ),
                    )
                }
                other @ (NetResponse::FinalizedApprovals(_)
                | NetResponse::Block(_)
                | NetResponse::FinalitySignature(_)
                | NetResponse::GossipedAddress(_)
                | NetResponse::BlockAndMetadataByHeight(_)
                | NetResponse::BlockAndDeploys(_)
                | NetResponse::BlockHeaderByHash(_)
                | NetResponse::BlockHeaderAndFinalitySignaturesByHeight(_)
                | NetResponse::BlockHeadersBatch(_)
                | NetResponse::FinalitySignatures(_)) => {
                    fatal!(effect_builder, "unexpected net response: {:?}", other).ignore()
                }
            },
            other @ (Event::ConsensusMessageIncoming(_)
            | Event::FinalitySignatureIncoming(_)
            | Event::FinalitySignatureGossiperIncoming(_)
            | Event::AddressGossiperIncoming(_)
            | Event::TrieRequestIncoming(_)
            | Event::TrieDemand(_)
            | Event::TrieResponseIncoming(_)
            | Event::SyncLeapRequestIncoming(_)
            | Event::SyncLeapResponseIncoming(_)
            | Event::BlockAddedRequestIncoming(_)
            | Event::BlockAddedResponseIncoming(_)) => {
                fatal!(effect_builder, "should not receive {:?}", other).ignore()
            }
        }
    }

    fn maybe_exit(&self) -> Option<crate::reactor::ReactorExit> {
        unimplemented!()
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
    let mut network = Network::<Reactor>::new();

    // Add `network_size` nodes.
    let node_ids = network.add_nodes(rng, network_size).await;

    // Create `deploy_count` random deploys.
    let (all_deploy_hashes, mut deploys): (BTreeSet<_>, Vec<_>) = iter::repeat_with(|| {
        let deploy = Box::new(Deploy::random_valid_native_transfer(rng));
        (*deploy.id(), deploy)
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
    let mut network = Network::<Reactor>::new();
    let mut rng = crate::new_rng();

    // Add `NETWORK_SIZE` nodes.
    let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;

    // Create random deploy.
    let deploy = Box::new(Deploy::random_valid_native_transfer(&mut rng));
    let deploy_id = *deploy.id();

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
    let mut network = Network::<Reactor>::new();
    let mut rng = crate::new_rng();

    // The target number of peers to infect with a given piece of data.
    let infection_target = Config::default().infection_target();

    // Add `infection_target + 1` nodes.
    let mut node_ids = network
        .add_nodes(&mut rng, infection_target as usize + 1)
        .await;

    // Create random deploy.
    let deploy = Box::new(Deploy::random_valid_native_transfer(&mut rng));
    let deploy_id = *deploy.id();

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
