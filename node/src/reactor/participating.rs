//! Reactor for participating nodes.
//!
//! Participating nodes join the participating-only network upon startup.

mod config;
mod error;
mod memory_metrics;
#[cfg(test)]
mod tests;

use std::{
    fmt::{self, Debug, Display, Formatter},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use casper_execution_engine::{
    core::engine_state::EngineState, storage::global_state::db::DbGlobalState,
};
use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tracing::{debug, error, info, trace, warn};

#[cfg(test)]
use crate::testing::network::NetworkedReactor;

use crate::{
    components::{
        block_proposer::{self, BlockProposer},
        block_validator::{self, BlockValidator},
        chainspec_loader::{self, ChainspecLoader},
        consensus::{self, EraSupervisor, HighwayProtocol},
        console::{self, Console},
        contract_runtime::{ContractRuntime, ContractRuntimeAnnouncement, ExecutionPreState},
        deploy_acceptor::{self, DeployAcceptor},
        event_stream_server::{self, EventStreamServer},
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        linear_chain,
        metrics::Metrics,
        rest_server::{self, RestServer},
        rpc_server::{self, RpcServer},
        small_network::{self, GossipedAddress, SmallNetwork, SmallNetworkIdentity},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            BlockProposerAnnouncement, BlocklistAnnouncement, ChainspecLoaderAnnouncement,
            ConsensusAnnouncement, ControlAnnouncement, DeployAcceptorAnnouncement,
            GossiperAnnouncement, LinearChainAnnouncement, LinearChainBlock, NetworkAnnouncement,
            RpcServerAnnouncement,
        },
        console::DumpConsensusStateRequest,
        requests::{
            BlockProposerRequest, BlockValidationRequest, ChainspecLoaderRequest, ConsensusRequest,
            ContractRuntimeRequest, FetcherRequest, LinearChainRequest, MetricsRequest,
            NetworkInfoRequest, NetworkRequest, RestRequest, RpcRequest, StateStoreRequest,
            StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    reactor::{
        self, event_queue_metrics::EventQueueMetrics, EventQueueHandle, Reactor as _, ReactorExit,
    },
    types::{BlockHash, BlockHeader, Deploy, ExitCode, NodeId, Tag},
    utils::{Source, WithDir},
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use error::Error;
use linear_chain::LinearChainComponent;
use memory_metrics::MemoryMetrics;

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
// Note: The large enum size must be reigned in eventually. This is a stopgap for now.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ParticipatingEvent {
    /// Small network event.
    #[from]
    SmallNetwork(small_network::Event<Message>),
    /// Block proposer event.
    #[from]
    BlockProposer(#[serde(skip_serializing)] block_proposer::Event),
    #[from]
    /// Storage event.
    Storage(#[serde(skip_serializing)] storage::Event),
    #[from]
    /// RPC server event.
    RpcServer(#[serde(skip_serializing)] rpc_server::Event),
    #[from]
    /// REST server event.
    RestServer(#[serde(skip_serializing)] rest_server::Event),
    #[from]
    /// Event stream server event.
    EventStreamServer(#[serde(skip_serializing)] event_stream_server::Event),
    #[from]
    /// Chainspec Loader event.
    ChainspecLoader(#[serde(skip_serializing)] chainspec_loader::Event),
    #[from]
    /// Consensus event.
    Consensus(#[serde(skip_serializing)] consensus::Event),
    /// Deploy acceptor event.
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] deploy_acceptor::Event),
    /// Deploy fetcher event.
    #[from]
    DeployFetcher(#[serde(skip_serializing)] fetcher::Event<Deploy>),
    /// Deploy gossiper event.
    #[from]
    DeployGossiper(#[serde(skip_serializing)] gossiper::Event<Deploy>),
    /// Address gossiper event.
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),
    /// Block validator event.
    #[from]
    BlockValidator(#[serde(skip_serializing)] block_validator::Event),
    /// Linear chain event.
    #[from]
    LinearChain(#[serde(skip_serializing)] linear_chain::Event),
    /// Console event.
    #[from]
    Console(console::Event),

    // Requests
    /// Contract runtime request.
    ContractRuntime(#[serde(skip_serializing)] Box<ContractRuntimeRequest>),
    /// Network request.
    #[from]
    NetworkRequest(#[serde(skip_serializing)] NetworkRequest<Message>),
    /// Network info request.
    #[from]
    NetworkInfoRequest(#[serde(skip_serializing)] NetworkInfoRequest),
    /// Deploy fetcher request.
    #[from]
    DeployFetcherRequest(#[serde(skip_serializing)] FetcherRequest<Deploy>),
    /// Block proposer request.
    #[from]
    BlockProposerRequest(#[serde(skip_serializing)] BlockProposerRequest),
    /// Block validator request.
    #[from]
    BlockValidatorRequest(#[serde(skip_serializing)] BlockValidationRequest),
    /// Metrics request.
    #[from]
    MetricsRequest(#[serde(skip_serializing)] MetricsRequest),
    /// Chainspec info request
    #[from]
    ChainspecLoaderRequest(#[serde(skip_serializing)] ChainspecLoaderRequest),
    /// Storage request.
    #[from]
    StorageRequest(#[serde(skip_serializing)] StorageRequest),
    /// Request for state storage.
    #[from]
    StateStoreRequest(StateStoreRequest),
    /// Consensus dump request.
    #[from]
    DumpConsensusStateRequest(DumpConsensusStateRequest),

    // Announcements
    /// Control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    /// Network announcement.
    #[from]
    NetworkAnnouncement(#[serde(skip_serializing)] NetworkAnnouncement<Message>),
    /// API server announcement.
    #[from]
    RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
    /// DeployAcceptor announcement.
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement),
    /// Consensus announcement.
    #[from]
    ConsensusAnnouncement(#[serde(skip_serializing)] ConsensusAnnouncement),
    /// ContractRuntime announcement.
    #[from]
    ContractRuntimeAnnouncement(#[serde(skip_serializing)] ContractRuntimeAnnouncement),
    /// Deploy Gossiper announcement.
    #[from]
    DeployGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<Deploy>),
    /// Address Gossiper announcement.
    #[from]
    AddressGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<GossipedAddress>),
    /// Linear chain announcement.
    #[from]
    LinearChainAnnouncement(#[serde(skip_serializing)] LinearChainAnnouncement),
    /// Chainspec loader announcement.
    #[from]
    ChainspecLoaderAnnouncement(#[serde(skip_serializing)] ChainspecLoaderAnnouncement),
    /// Blocklist announcement.
    #[from]
    BlocklistAnnouncement(BlocklistAnnouncement),
    /// Block proposer announcement.
    #[from]
    BlockProposerAnnouncement(#[serde(skip_serializing)] BlockProposerAnnouncement),
}

impl ReactorEvent for ParticipatingEvent {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    #[inline]
    fn description(&self) -> &'static str {
        match self {
            ParticipatingEvent::SmallNetwork(_) => "SmallNetwork",
            ParticipatingEvent::BlockProposer(_) => "BlockProposer",
            ParticipatingEvent::Storage(_) => "Storage",
            ParticipatingEvent::RpcServer(_) => "RpcServer",
            ParticipatingEvent::RestServer(_) => "RestServer",
            ParticipatingEvent::EventStreamServer(_) => "EventStreamServer",
            ParticipatingEvent::ChainspecLoader(_) => "ChainspecLoader",
            ParticipatingEvent::Consensus(_) => "Consensus",
            ParticipatingEvent::DeployAcceptor(_) => "DeployAcceptor",
            ParticipatingEvent::DeployFetcher(_) => "DeployFetcher",
            ParticipatingEvent::DeployGossiper(_) => "DeployGossiper",
            ParticipatingEvent::AddressGossiper(_) => "AddressGossiper",
            ParticipatingEvent::BlockValidator(_) => "BlockValidator",
            ParticipatingEvent::LinearChain(_) => "LinearChain",
            ParticipatingEvent::ContractRuntime(_) => "ContractRuntime",
            ParticipatingEvent::Console(_) => "Console",
            ParticipatingEvent::NetworkRequest(_) => "NetworkRequest",
            ParticipatingEvent::NetworkInfoRequest(_) => "NetworkInfoRequest",
            ParticipatingEvent::DeployFetcherRequest(_) => "DeployFetcherRequest",
            ParticipatingEvent::BlockProposerRequest(_) => "BlockProposerRequest",
            ParticipatingEvent::BlockValidatorRequest(_) => "BlockValidatorRequest",
            ParticipatingEvent::MetricsRequest(_) => "MetricsRequest",
            ParticipatingEvent::ChainspecLoaderRequest(_) => "ChainspecLoaderRequest",
            ParticipatingEvent::StorageRequest(_) => "StorageRequest",
            ParticipatingEvent::StateStoreRequest(_) => "StateStoreRequest",
            ParticipatingEvent::DumpConsensusStateRequest(_) => "DumpConsensusStateRequest",
            ParticipatingEvent::ControlAnnouncement(_) => "ControlAnnouncement",
            ParticipatingEvent::NetworkAnnouncement(_) => "NetworkAnnouncement",
            ParticipatingEvent::RpcServerAnnouncement(_) => "RpcServerAnnouncement",
            ParticipatingEvent::DeployAcceptorAnnouncement(_) => "DeployAcceptorAnnouncement",
            ParticipatingEvent::ConsensusAnnouncement(_) => "ConsensusAnnouncement",
            ParticipatingEvent::ContractRuntimeAnnouncement(_) => "ContractRuntimeAnnouncement",
            ParticipatingEvent::DeployGossiperAnnouncement(_) => "DeployGossiperAnnouncement",
            ParticipatingEvent::AddressGossiperAnnouncement(_) => "AddressGossiperAnnouncement",
            ParticipatingEvent::LinearChainAnnouncement(_) => "LinearChainAnnouncement",
            ParticipatingEvent::ChainspecLoaderAnnouncement(_) => "ChainspecLoaderAnnouncement",
            ParticipatingEvent::BlocklistAnnouncement(_) => "BlocklistAnnouncement",
            ParticipatingEvent::BlockProposerAnnouncement(_) => "BlockProposerAnnouncement",
        }
    }
}

impl From<ContractRuntimeRequest> for ParticipatingEvent {
    fn from(contract_runtime_request: ContractRuntimeRequest) -> Self {
        ParticipatingEvent::ContractRuntime(Box::new(contract_runtime_request))
    }
}

impl From<RpcRequest> for ParticipatingEvent {
    fn from(request: RpcRequest) -> Self {
        ParticipatingEvent::RpcServer(rpc_server::Event::RpcRequest(request))
    }
}

impl From<RestRequest> for ParticipatingEvent {
    fn from(request: RestRequest) -> Self {
        ParticipatingEvent::RestServer(rest_server::Event::RestRequest(request))
    }
}

impl From<NetworkRequest<consensus::ConsensusMessage>> for ParticipatingEvent {
    fn from(request: NetworkRequest<consensus::ConsensusMessage>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<Deploy>>> for ParticipatingEvent {
    fn from(request: NetworkRequest<gossiper::Message<Deploy>>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<GossipedAddress>>> for ParticipatingEvent {
    fn from(request: NetworkRequest<gossiper::Message<GossipedAddress>>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<ConsensusRequest> for ParticipatingEvent {
    fn from(request: ConsensusRequest) -> Self {
        ParticipatingEvent::Consensus(consensus::Event::ConsensusRequest(request))
    }
}

impl From<LinearChainRequest> for ParticipatingEvent {
    fn from(request: LinearChainRequest) -> Self {
        ParticipatingEvent::LinearChain(linear_chain::Event::Request(request))
    }
}

impl Display for ParticipatingEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ParticipatingEvent::SmallNetwork(event) => write!(f, "small network: {}", event),
            ParticipatingEvent::BlockProposer(event) => write!(f, "block proposer: {}", event),
            ParticipatingEvent::Storage(event) => write!(f, "storage: {}", event),
            ParticipatingEvent::RpcServer(event) => write!(f, "rpc server: {}", event),
            ParticipatingEvent::RestServer(event) => write!(f, "rest server: {}", event),
            ParticipatingEvent::EventStreamServer(event) => {
                write!(f, "event stream server: {}", event)
            }
            ParticipatingEvent::ChainspecLoader(event) => write!(f, "chainspec loader: {}", event),
            ParticipatingEvent::Consensus(event) => write!(f, "consensus: {}", event),
            ParticipatingEvent::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            ParticipatingEvent::DeployFetcher(event) => write!(f, "deploy fetcher: {}", event),
            ParticipatingEvent::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            ParticipatingEvent::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            ParticipatingEvent::ContractRuntime(event) => {
                write!(f, "contract runtime: {:?}", event)
            }
            ParticipatingEvent::LinearChain(event) => write!(f, "linear-chain event {}", event),
            ParticipatingEvent::BlockValidator(event) => write!(f, "block validator: {}", event),
            ParticipatingEvent::Console(event) => write!(f, "console: {}", event),
            ParticipatingEvent::NetworkRequest(req) => write!(f, "network request: {}", req),
            ParticipatingEvent::NetworkInfoRequest(req) => {
                write!(f, "network info request: {}", req)
            }
            ParticipatingEvent::ChainspecLoaderRequest(req) => {
                write!(f, "chainspec loader request: {}", req)
            }
            ParticipatingEvent::StorageRequest(req) => write!(f, "storage request: {}", req),
            ParticipatingEvent::StateStoreRequest(req) => write!(f, "state store request: {}", req),
            ParticipatingEvent::DeployFetcherRequest(req) => {
                write!(f, "deploy fetcher request: {}", req)
            }
            ParticipatingEvent::BlockProposerRequest(req) => {
                write!(f, "block proposer request: {}", req)
            }
            ParticipatingEvent::BlockValidatorRequest(req) => {
                write!(f, "block validator request: {}", req)
            }
            ParticipatingEvent::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            ParticipatingEvent::ControlAnnouncement(ctrl_ann) => write!(f, "control: {}", ctrl_ann),
            ParticipatingEvent::DumpConsensusStateRequest(req) => {
                write!(f, "dump consensus state: {}", req)
            }
            ParticipatingEvent::NetworkAnnouncement(ann) => {
                write!(f, "network announcement: {}", ann)
            }
            ParticipatingEvent::RpcServerAnnouncement(ann) => {
                write!(f, "api server announcement: {}", ann)
            }
            ParticipatingEvent::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            ParticipatingEvent::ConsensusAnnouncement(ann) => {
                write!(f, "consensus announcement: {}", ann)
            }
            ParticipatingEvent::ContractRuntimeAnnouncement(ann) => {
                write!(f, "block-executor announcement: {}", ann)
            }
            ParticipatingEvent::DeployGossiperAnnouncement(ann) => {
                write!(f, "deploy gossiper announcement: {}", ann)
            }
            ParticipatingEvent::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
            ParticipatingEvent::LinearChainAnnouncement(ann) => {
                write!(f, "linear chain announcement: {}", ann)
            }
            ParticipatingEvent::BlockProposerAnnouncement(ann) => {
                write!(f, "block proposer announcement: {}", ann)
            }
            ParticipatingEvent::ChainspecLoaderAnnouncement(ann) => {
                write!(f, "chainspec loader announcement: {}", ann)
            }
            ParticipatingEvent::BlocklistAnnouncement(ann) => {
                write!(f, "blocklist announcement: {}", ann)
            }
        }
    }
}

/// The configuration needed to initialize a Participating reactor
pub(crate) struct ParticipatingInitConfig {
    pub(super) root: PathBuf,
    pub(super) config: Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
    pub(super) maybe_latest_block_header: Option<BlockHeader>,
    pub(super) event_stream_server: EventStreamServer,
    pub(super) small_network_identity: SmallNetworkIdentity,
    pub(super) node_startup_instant: Instant,
}

#[cfg(test)]
impl ParticipatingInitConfig {
    /// Inspect storage.
    pub(crate) fn storage(&self) -> &Storage {
        &self.storage
    }
}

impl Debug for ParticipatingInitConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "ParticipatingInitConfig {{ .. }}")
    }
}

/// Participating node reactor.
#[derive(DataSize, Debug)]
pub(crate) struct Reactor {
    metrics: Metrics,
    small_network: SmallNetwork<ParticipatingEvent, Message>,
    address_gossiper: Gossiper<GossipedAddress, ParticipatingEvent>,
    storage: Storage,
    contract_runtime: ContractRuntime,
    rpc_server: RpcServer,
    rest_server: RestServer,
    event_stream_server: EventStreamServer,
    chainspec_loader: ChainspecLoader,
    consensus: EraSupervisor,
    #[data_size(skip)]
    deploy_acceptor: DeployAcceptor,
    deploy_fetcher: Fetcher<Deploy>,
    deploy_gossiper: Gossiper<Deploy, ParticipatingEvent>,
    block_proposer: BlockProposer,
    block_validator: BlockValidator,
    linear_chain: LinearChainComponent,
    console: Console,

    // Non-components.
    #[data_size(skip)] // Never allocates heap data.
    memory_metrics: MemoryMetrics,

    #[data_size(skip)]
    event_queue_metrics: EventQueueMetrics,
}

#[cfg(test)]
impl Reactor {
    /// Inspect consensus.
    pub(crate) fn consensus(&self) -> &EraSupervisor {
        &self.consensus
    }

    /// Inspect storage.
    pub(crate) fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Inspect contract runtime.
    pub(crate) fn contract_runtime(&self) -> &ContractRuntime {
        &self.contract_runtime
    }
}

impl Reactor {
    /// Handles a request to get an item by id.
    fn handle_get_request(
        &mut self,
        effect_builder: EffectBuilder<<Self as reactor::Reactor>::Event>,
        rng: &mut NodeRng,
        sender: NodeId,
        tag: Tag,
        serialized_id: &[u8],
    ) -> Effects<<Self as reactor::Reactor>::Event> {
        match tag {
            Tag::Deploy => {
                let deploy_hash = match bincode::deserialize(serialized_id) {
                    Ok(hash) => hash,
                    Err(error) => {
                        error!(
                            "failed to decode {:?} from {}: {}",
                            serialized_id, sender, error
                        );
                        return Effects::new();
                    }
                };

                match self
                    .storage
                    .handle_deduplicated_legacy_direct_deploy_request(deploy_hash)
                {
                    Some(serialized_item) => {
                        let message =
                            Message::new_get_response_raw_unchecked::<Deploy>(serialized_item);
                        return effect_builder.send_message(sender, message).ignore();
                    }
                    None => debug!(%sender, %deploy_hash, "failed to get deploy (not found)"),
                }
            }
            Tag::Block => match bincode::deserialize(serialized_id) {
                Ok(block_hash) => {
                    let req = LinearChainRequest::BlockRequest(block_hash, sender);
                    let event = ParticipatingEvent::LinearChain(linear_chain::Event::Request(req));
                    return self.dispatch_event(effect_builder, rng, event);
                }
                Err(error) => error!(
                    "failed to decode {:?} from {}: {}",
                    serialized_id, sender, error
                ),
            },
            Tag::BlockByHeight => match bincode::deserialize(serialized_id) {
                Ok(height) => {
                    let req = LinearChainRequest::BlockAtHeight(height, sender);
                    let event = ParticipatingEvent::LinearChain(linear_chain::Event::Request(req));
                    return self.dispatch_event(effect_builder, rng, event);
                }
                Err(error) => error!(
                    "failed to decode {:?} from {}: {}",
                    serialized_id, sender, error
                ),
            },
            Tag::GossipedAddress => {
                warn!("received get request for gossiped-address from {}", sender)
            }
            Tag::BlockHeaderByHash => {
                let block_hash: BlockHash = match bincode::deserialize(serialized_id) {
                    Ok(block_hash) => block_hash,
                    Err(error) => {
                        error!(
                            "failed to decode {:?} from {}: {}",
                            serialized_id, sender, error
                        );
                        return Effects::new();
                    }
                };

                match self.storage.get_block_header_by_hash(&block_hash) {
                    Ok(Some(block_header)) => {
                        match Message::new_get_response(&block_header) {
                            Err(error) => error!("failed to create get-response: {}", error),
                            Ok(message) => {
                                return effect_builder.send_message(sender, message).ignore();
                            }
                        };
                    }
                    Ok(None) => debug!("failed to get {} for {}", block_hash, sender),
                    Err(error) => error!("failed to get {} for {}: {}", block_hash, sender, error),
                }
            }
            Tag::BlockHeaderAndFinalitySignaturesByHeight => {
                let block_height = match bincode::deserialize(serialized_id) {
                    Ok(block_height) => block_height,
                    Err(error) => {
                        error!(
                            "failed to decode {:?} from {}: {}",
                            serialized_id, sender, error
                        );
                        return Effects::new();
                    }
                };
                match self
                    .storage
                    .read_block_header_and_finality_signatures_by_height(block_height)
                {
                    Ok(Some(block_header)) => {
                        match Message::new_get_response(&block_header) {
                            Ok(message) => {
                                return effect_builder.send_message(sender, message).ignore();
                            }
                            Err(error) => error!("failed to create get-response: {}", error),
                        };
                    }
                    Ok(None) => debug!("failed to get {} for {}", block_height, sender),
                    Err(error) => {
                        error!("failed to get {} for {}: {}", block_height, sender, error)
                    }
                }
            }
        }
        Effects::new()
    }
}

impl reactor::Reactor for Reactor {
    type Event = ParticipatingEvent;

    // The "configuration" is in fact the whole state of the joiner reactor, which we
    // deconstruct and reuse.
    type Config = ParticipatingInitConfig;
    type Error = Error;

    fn new(
        config: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<ParticipatingEvent>), Error> {
        let ParticipatingInitConfig {
            root,
            config,
            chainspec_loader,
            storage,
            mut contract_runtime,
            maybe_latest_block_header,
            event_stream_server,
            small_network_identity,
            node_startup_instant,
        } = config;

        let memory_metrics = MemoryMetrics::new(registry.clone())?;

        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let metrics = Metrics::new(registry.clone());

        let (console, console_effects) =
            Console::new(&WithDir::new(&root, config.console.clone()), event_queue)?;

        let effect_builder = EffectBuilder::new(event_queue);

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let protocol_version = &chainspec_loader.chainspec().protocol_config.version;
        let rpc_server = RpcServer::new(
            config.rpc_server.clone(),
            effect_builder,
            *protocol_version,
            node_startup_instant,
        )?;
        let rest_server = RestServer::new(
            config.rest_server.clone(),
            effect_builder,
            *protocol_version,
            node_startup_instant,
        )?;

        let deploy_acceptor = DeployAcceptor::new(
            config.deploy_acceptor,
            &*chainspec_loader.chainspec(),
            registry,
        )?;

        let deploy_fetcher = Fetcher::new("deploy", config.fetcher, registry)?;
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config.gossip,
            gossiper::get_deploy_from_storage::<Deploy, ParticipatingEvent>,
            registry,
        )?;
        let (block_proposer, block_proposer_effects) = BlockProposer::new(
            registry.clone(),
            effect_builder,
            maybe_latest_block_header
                .as_ref()
                .map(|block_header| block_header.height() + 1)
                .unwrap_or(0),
            chainspec_loader.chainspec().as_ref(),
            config.block_proposer,
        )?;

        let initial_era = maybe_latest_block_header.as_ref().map_or_else(
            || chainspec_loader.initial_era(),
            |block_header| block_header.next_block_era_id(),
        );

        let (small_network, small_network_effects) = SmallNetwork::new(
            event_queue,
            config.network,
            Some(WithDir::new(&root, &config.consensus)),
            registry,
            small_network_identity,
            chainspec_loader.chainspec().as_ref(),
        )?;

        let mut effects =
            reactor::wrap_effects(ParticipatingEvent::BlockProposer, block_proposer_effects);

        effects.extend(reactor::wrap_effects(
            ParticipatingEvent::Console,
            console_effects,
        ));

        let maybe_next_activation_point = chainspec_loader
            .next_upgrade()
            .map(|next_upgrade| next_upgrade.activation_point());
        let (consensus, init_consensus_effects) = EraSupervisor::new(
            initial_era,
            storage.root_path(),
            WithDir::new(root, config.consensus),
            effect_builder,
            chainspec_loader.chainspec().as_ref().into(),
            maybe_latest_block_header.as_ref(),
            maybe_next_activation_point,
            registry,
            Box::new(HighwayProtocol::new_boxed),
        )?;
        effects.extend(reactor::wrap_effects(
            ParticipatingEvent::Consensus,
            init_consensus_effects,
        ));

        let execution_pre_state = match maybe_latest_block_header {
            // if there is a latest block header and it's later than the block that was highest
            // when the node was started up, we should use its post-state-hash as the initial state
            // hash
            Some(latest_block_header)
                if latest_block_header.height()
                    >= chainspec_loader
                        .initial_execution_pre_state()
                        .next_block_height() =>
            {
                ExecutionPreState::from(&latest_block_header)
            }
            _ => chainspec_loader.initial_execution_pre_state(),
        };

        // Kick off migration of data from lmdb to rocksdb in a background task.
        {
            let engine_state = Arc::clone(contract_runtime.engine_state());
            let storage = storage.clone();
            let background_migration = tokio::task::spawn_blocking(move || {
                migrate_lmdb_data_to_rocksdb(engine_state, storage, true)
            });

            effects.extend(background_migration.ignore());
        }

        contract_runtime.set_initial_state(execution_pre_state);

        let block_validator = BlockValidator::new(Arc::clone(chainspec_loader.chainspec()));
        let linear_chain = linear_chain::LinearChainComponent::new(
            registry,
            *protocol_version,
            chainspec_loader.chainspec().core_config.auction_delay,
            chainspec_loader.chainspec().core_config.unbonding_delay,
        )?;

        effects.extend(reactor::wrap_effects(
            ParticipatingEvent::SmallNetwork,
            small_network_effects,
        ));
        effects.extend(reactor::wrap_effects(
            ParticipatingEvent::ChainspecLoader,
            chainspec_loader.start_checking_for_upgrades(effect_builder),
        ));

        event_stream_server.set_participating_effect_builder(effect_builder);

        Ok((
            Reactor {
                metrics,
                small_network,
                address_gossiper,
                storage,
                contract_runtime,
                rpc_server,
                rest_server,
                event_stream_server,
                chainspec_loader,
                consensus,
                deploy_acceptor,
                deploy_fetcher,
                deploy_gossiper,
                block_proposer,
                block_validator,
                linear_chain,
                console,
                memory_metrics,
                event_queue_metrics,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: ParticipatingEvent,
    ) -> Effects<Self::Event> {
        match event {
            ParticipatingEvent::SmallNetwork(event) => reactor::wrap_effects(
                ParticipatingEvent::SmallNetwork,
                self.small_network.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockProposer(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockProposer,
                self.block_proposer.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::Storage(event) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::RpcServer(event) => reactor::wrap_effects(
                ParticipatingEvent::RpcServer,
                self.rpc_server.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::RestServer(event) => reactor::wrap_effects(
                ParticipatingEvent::RestServer,
                self.rest_server.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::EventStreamServer(event) => reactor::wrap_effects(
                ParticipatingEvent::EventStreamServer,
                self.event_stream_server
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::ChainspecLoader(event) => reactor::wrap_effects(
                ParticipatingEvent::ChainspecLoader,
                self.chainspec_loader
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::Consensus(event) => reactor::wrap_effects(
                ParticipatingEvent::Consensus,
                self.consensus.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::DeployAcceptor(event) => reactor::wrap_effects(
                ParticipatingEvent::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::DeployFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::DeployFetcher,
                self.deploy_fetcher.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::DeployGossiper(event) => reactor::wrap_effects(
                ParticipatingEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::AddressGossiper(event) => reactor::wrap_effects(
                ParticipatingEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::ContractRuntime(event) => reactor::wrap_effects(
                Into::into,
                self.contract_runtime
                    .handle_event(effect_builder, rng, *event),
            ),
            ParticipatingEvent::BlockValidator(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockValidator,
                self.block_validator
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::LinearChain(event) => reactor::wrap_effects(
                ParticipatingEvent::LinearChain,
                self.linear_chain.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::Console(event) => reactor::wrap_effects(
                ParticipatingEvent::Console,
                self.console.handle_event(effect_builder, rng, event),
            ),

            // Requests:
            ParticipatingEvent::NetworkRequest(req) => {
                let event = ParticipatingEvent::SmallNetwork(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            ParticipatingEvent::NetworkInfoRequest(req) => {
                let event = ParticipatingEvent::SmallNetwork(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            ParticipatingEvent::DeployFetcherRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::DeployFetcher(req.into()),
            ),
            ParticipatingEvent::BlockProposerRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::BlockProposer(req.into()),
            ),
            ParticipatingEvent::BlockValidatorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::BlockValidator(block_validator::Event::from(req)),
            ),
            ParticipatingEvent::MetricsRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::MetricsRequest,
                self.metrics.handle_event(effect_builder, rng, req),
            ),
            ParticipatingEvent::ChainspecLoaderRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::ChainspecLoader(req.into()),
            ),
            ParticipatingEvent::StorageRequest(req) => {
                self.dispatch_event(effect_builder, rng, ParticipatingEvent::Storage(req.into()))
            }
            ParticipatingEvent::StateStoreRequest(req) => {
                self.dispatch_event(effect_builder, rng, ParticipatingEvent::Storage(req.into()))
            }
            ParticipatingEvent::DumpConsensusStateRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::Consensus,
                self.consensus.handle_event(effect_builder, rng, req.into()),
            ),

            // Announcements:
            ParticipatingEvent::ControlAnnouncement(ctrl_ann) => {
                unreachable!("unhandled control announcement: {}", ctrl_ann)
            }
            ParticipatingEvent::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => {
                let reactor_event = match payload {
                    Message::Consensus(msg) => {
                        ParticipatingEvent::Consensus(consensus::Event::MessageReceived {
                            sender,
                            msg,
                        })
                    }
                    Message::DeployGossiper(message) => {
                        ParticipatingEvent::DeployGossiper(gossiper::Event::MessageReceived {
                            sender,
                            message,
                        })
                    }
                    Message::AddressGossiper(message) => {
                        ParticipatingEvent::AddressGossiper(gossiper::Event::MessageReceived {
                            sender,
                            message,
                        })
                    }
                    Message::GetRequest { tag, serialized_id } => {
                        return self.handle_get_request(
                            effect_builder,
                            rng,
                            sender,
                            tag,
                            &serialized_id,
                        )
                    }
                    Message::GetResponse {
                        tag,
                        serialized_item,
                    } => match tag {
                        Tag::Deploy => {
                            let deploy = match bincode::deserialize(&serialized_item) {
                                Ok(deploy) => Box::new(deploy),
                                Err(error) => {
                                    error!("failed to decode deploy from {}: {}", sender, error);
                                    return Effects::new();
                                }
                            };
                            ParticipatingEvent::DeployAcceptor(deploy_acceptor::Event::Accept {
                                deploy,
                                source: Source::Peer(sender),
                                maybe_responder: None,
                            })
                        }
                        Tag::Block => {
                            error!(
                                "cannot handle get response for block-by-hash from {}",
                                sender
                            );
                            return Effects::new();
                        }
                        Tag::BlockByHeight => {
                            error!(
                                "cannot handle get response for block-by-height from {}",
                                sender
                            );
                            return Effects::new();
                        }
                        Tag::GossipedAddress => {
                            error!(
                                "cannot handle get response for gossiped-address from {}",
                                sender
                            );
                            return Effects::new();
                        }
                        Tag::BlockHeaderByHash => {
                            error!(
                                "cannot handle get response for block-header-by-hash from {}",
                                sender
                            );
                            return Effects::new();
                        }
                        Tag::BlockHeaderAndFinalitySignaturesByHeight => {
                            error!(
                                "cannot handle get response for \
                                 block-header-and-finality-signatures-by-height from {}",
                                sender
                            );
                            return Effects::new();
                        }
                    },
                    Message::FinalitySignature(fs) => ParticipatingEvent::LinearChain(
                        linear_chain::Event::FinalitySignatureReceived(fs, true),
                    ),
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            ParticipatingEvent::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(
                gossiped_address,
            )) => {
                let event = gossiper::Event::ItemReceived {
                    item_id: gossiped_address,
                    source: Source::Ourself,
                };
                self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::AddressGossiper(event),
                )
            }
            ParticipatingEvent::NetworkAnnouncement(NetworkAnnouncement::NewPeer(_peer_id)) => {
                trace!("new peer announcement not handled in the participating reactor");
                Effects::new()
            }
            ParticipatingEvent::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived {
                deploy,
                responder,
            }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::Client,
                    maybe_responder: responder,
                };
                self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::DeployAcceptor(event),
                )
            }
            ParticipatingEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            ) => {
                let deploy_info = match deploy.deploy_info() {
                    Ok(deploy_info) => deploy_info,
                    Err(error) => {
                        error!(%error, "invalid deploy");
                        return Effects::new();
                    }
                };

                let event = block_proposer::Event::BufferDeploy {
                    hash: deploy.deploy_or_transfer_hash(),
                    deploy_info: Box::new(deploy_info),
                };
                let mut effects = self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::BlockProposer(event),
                );

                let event = gossiper::Event::ItemReceived {
                    item_id: *deploy.id(),
                    source: source.clone(),
                };
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::DeployGossiper(event),
                ));

                let event = event_stream_server::Event::DeployAccepted(*deploy.id());
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::EventStreamServer(event),
                ));

                let event = fetcher::Event::GotRemotely {
                    item: deploy,
                    source,
                };
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    ParticipatingEvent::DeployFetcher(event),
                ));

                effects
            }
            ParticipatingEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::InvalidDeploy {
                    deploy: _,
                    source: _,
                },
            ) => Effects::new(),
            ParticipatingEvent::ConsensusAnnouncement(consensus_announcement) => {
                match consensus_announcement {
                    ConsensusAnnouncement::Finalized(block) => {
                        let reactor_event = ParticipatingEvent::BlockProposer(
                            block_proposer::Event::FinalizedBlock(block),
                        );
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                    ConsensusAnnouncement::CreatedFinalitySignature(fs) => self.dispatch_event(
                        effect_builder,
                        rng,
                        ParticipatingEvent::LinearChain(
                            linear_chain::Event::FinalitySignatureReceived(fs, false),
                        ),
                    ),
                    ConsensusAnnouncement::Fault {
                        era_id,
                        public_key,
                        timestamp,
                    } => {
                        let reactor_event = ParticipatingEvent::EventStreamServer(
                            event_stream_server::Event::Fault {
                                era_id,
                                public_key: *public_key,
                                timestamp,
                            },
                        );
                        self.dispatch_event(effect_builder, rng, reactor_event)
                    }
                }
            }
            ParticipatingEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::LinearChainBlock(linear_chain_block),
            ) => {
                let LinearChainBlock {
                    block,
                    execution_results,
                } = *linear_chain_block;
                let mut effects = Effects::new();
                let block_hash = *block.hash();

                // send to linear chain
                let reactor_event =
                    ParticipatingEvent::LinearChain(linear_chain::Event::NewLinearChainBlock {
                        block: Box::new(block),
                        execution_results: execution_results
                            .iter()
                            .map(|(hash, _header, results)| (*hash, results.clone()))
                            .collect(),
                    });
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));

                // send to event stream
                for (deploy_hash, deploy_header, execution_result) in execution_results {
                    let reactor_event = ParticipatingEvent::EventStreamServer(
                        event_stream_server::Event::DeployProcessed {
                            deploy_hash,
                            deploy_header: Box::new(deploy_header),
                            block_hash,
                            execution_result: Box::new(execution_result),
                        },
                    );
                    effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                }

                effects
            }
            ParticipatingEvent::ContractRuntimeAnnouncement(
                ContractRuntimeAnnouncement::StepSuccess {
                    era_id,
                    execution_effect,
                },
            ) => {
                let reactor_event =
                    ParticipatingEvent::EventStreamServer(event_stream_server::Event::Step {
                        era_id,
                        execution_effect,
                    });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            ParticipatingEvent::DeployGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_deploy_id),
            ) => {
                error!(%gossiped_deploy_id, "gossiper should not announce new deploy");
                Effects::new()
            }
            ParticipatingEvent::DeployGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(_gossiped_deploy_id),
            ) => {
                // let reactor_event =
                //     ParticipatingEvent::BlockProposer(block_proposer::Event::
                // BufferDeploy(gossiped_deploy_id));
                // self.dispatch_event(effect_builder, rng, reactor_event)
                Effects::new()
            }
            ParticipatingEvent::AddressGossiperAnnouncement(
                GossiperAnnouncement::NewCompleteItem(gossiped_address),
            ) => {
                let reactor_event = ParticipatingEvent::SmallNetwork(
                    small_network::Event::PeerAddressReceived(gossiped_address),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            ParticipatingEvent::AddressGossiperAnnouncement(
                GossiperAnnouncement::FinishedGossiping(_),
            ) => {
                // We don't care about completion of gossiping an address.
                Effects::new()
            }
            ParticipatingEvent::LinearChainAnnouncement(LinearChainAnnouncement::BlockAdded(
                block,
            )) => {
                let reactor_event_consensus = ParticipatingEvent::Consensus(
                    consensus::Event::BlockAdded(Box::new(block.header().clone())),
                );
                let reactor_event_es = ParticipatingEvent::EventStreamServer(
                    event_stream_server::Event::BlockAdded(block),
                );
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event_es);
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event_consensus));

                effects
            }
            ParticipatingEvent::BlockProposerAnnouncement(
                BlockProposerAnnouncement::DeploysExpired(hashes),
            ) => {
                let reactor_event = ParticipatingEvent::EventStreamServer(
                    event_stream_server::Event::DeploysExpired(hashes),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            ParticipatingEvent::LinearChainAnnouncement(
                LinearChainAnnouncement::NewFinalitySignature(fs),
            ) => {
                let reactor_event = ParticipatingEvent::EventStreamServer(
                    event_stream_server::Event::FinalitySignature(fs),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            ParticipatingEvent::ChainspecLoaderAnnouncement(
                ChainspecLoaderAnnouncement::UpgradeActivationPointRead(next_upgrade),
            ) => {
                let reactor_event = ParticipatingEvent::ChainspecLoader(
                    chainspec_loader::Event::GotNextUpgrade(next_upgrade.clone()),
                );
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event);

                let reactor_event = ParticipatingEvent::Consensus(
                    consensus::Event::GotUpgradeActivationPoint(next_upgrade.activation_point()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                effects
            }
            ParticipatingEvent::BlocklistAnnouncement(ann) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::SmallNetwork(ann.into()),
            ),
            ParticipatingEvent::ContractRuntimeAnnouncement(ann) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::SmallNetwork(ann.into()),
            ),
        }
    }

    fn update_metrics(&mut self, event_queue_handle: EventQueueHandle<Self::Event>) {
        self.memory_metrics.estimate(self);
        self.event_queue_metrics
            .record_event_queue_counts(&event_queue_handle)
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        self.consensus
            .stop_for_upgrade()
            .then(|| ReactorExit::ProcessShouldExit(ExitCode::Success))
    }
}

/// Migrate data from lmdb to rocksdb.
pub fn migrate_lmdb_data_to_rocksdb(
    engine_state: Arc<EngineState<DbGlobalState>>,
    storage: Storage,
    limit_rate: bool,
) {
    let highest_block_height = match storage.read_highest_block_header() {
        Ok(Some(highest_block_header)) => highest_block_header.height(),
        Ok(None) => {
            info!("didn't find a highest block, will not migrate from lmdb to rocksdb");
            return;
        }
        Err(err) => {
            error!(?err, "unable to retrieve highest block from storage");
            return;
        }
    };

    let mut total_state_roots_migrated = 0;

    for height in (0..=highest_block_height).rev() {
        let start = Instant::now();
        let block_header = match storage.read_block_by_height(height) {
            Ok(Some(block)) => block.header().clone(),
            Ok(None) => {
                error!(
                    "unable to retrieve block at height {} during migration to rocksdb",
                    height,
                );
                continue;
            }
            Err(err) => {
                error!(
                    "unable to retrieve parent block at height {} during migration to rocksdb {:?}",
                    height, err,
                );
                continue;
            }
        };

        match engine_state
            .migrate_state_root_to_rocksdb_if_needed(*block_header.state_root_hash(), limit_rate)
        {
            Ok(true) => {
                info!(
                    block_height = %block_header.height(),
                    state_root_hash = %block_header.state_root_hash(),
                    time_migration_took_millis = %start.elapsed().as_millis(),
                    "successfully state root migrated from lmdb to rocksdb",
                );
                total_state_roots_migrated += 1;
            }
            Ok(false) => debug!(
                state_root = %block_header.state_root_hash(),
                block_height = %block_header.height(),
                "state root already migrated",
            ),
            Err(err) => {
                error!(?err, "error migrating state root");
            }
        }
    }

    info!(
        %total_state_roots_migrated,
        %highest_block_height,
        "Migration from lmdb to rocksdb completed.",
    );
}

#[cfg(test)]
impl NetworkedReactor for Reactor {
    fn node_id(&self) -> NodeId {
        self.small_network.node_id()
    }
}
