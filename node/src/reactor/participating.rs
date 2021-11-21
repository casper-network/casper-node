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

use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tracing::{debug, error, info, trace};

#[cfg(test)]
use crate::testing::network::NetworkedReactor;

use crate::{
    components::{
        block_proposer::{self, BlockProposer},
        block_validator::{self, BlockValidator},
        chainspec_loader::{self, ChainspecLoader},
        consensus::{self, EraReport, EraSupervisor, HighwayProtocol},
        contract_runtime::{ContractRuntime, ExecutionPreState},
        deploy_acceptor::{self, DeployAcceptor},
        event_stream_server::{self, EventStreamServer},
        fetcher::{self, FetchedOrNotFound, Fetcher},
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
            ConsensusAnnouncement, ContractRuntimeAnnouncement, ControlAnnouncement,
            DeployAcceptorAnnouncement, GossiperAnnouncement, LinearChainAnnouncement,
            LinearChainBlock, RpcServerAnnouncement,
        },
        incoming::{
            ConsensusMessageIncoming, FinalitySignatureIncoming, GossiperIncoming,
            NetRequestIncoming, NetResponse, NetResponseIncoming, TrieRequest, TrieRequestIncoming,
            TrieResponseIncoming,
        },
        requests::{
            BeginGossipRequest, BlockProposerRequest, BlockValidationRequest,
            ChainspecLoaderRequest, ConsensusRequest, ContractRuntimeRequest, FetcherRequest,
            MetricsRequest, NetworkInfoRequest, NetworkRequest, RestRequest, RpcRequest,
            StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    reactor::{self, event_queue_metrics::EventQueueMetrics, EventQueueHandle, ReactorExit},
    types::{
        ActivationPoint, BlockHeader, BlockPayload, Deploy, DeployHash, ExitCode, FinalizedBlock,
        Item, NodeId, Tag,
    },
    utils::{Source, WithDir},
    NodeRng,
};
use casper_execution_engine::core::engine_state::{GenesisSuccess, UpgradeConfig, UpgradeSuccess};
use casper_types::{EraId, ProtocolVersion, PublicKey};
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
    /// Storage event.
    #[from]
    Storage(storage::Event),
    /// Block proposer event.
    #[from]
    BlockProposer(#[serde(skip_serializing)] block_proposer::Event),
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
    Consensus(#[serde(skip_serializing)] consensus::Event<NodeId>),
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
    BlockValidator(#[serde(skip_serializing)] block_validator::Event<NodeId>),
    /// Linear chain event.
    #[from]
    LinearChain(#[serde(skip_serializing)] linear_chain::Event),

    // Requests
    /// Contract runtime request.
    ContractRuntime(#[serde(skip_serializing)] Box<ContractRuntimeRequest>),
    /// Network request.
    #[from]
    NetworkRequest(#[serde(skip_serializing)] NetworkRequest<NodeId, Message>),
    /// Network info request.
    #[from]
    NetworkInfoRequest(#[serde(skip_serializing)] NetworkInfoRequest<NodeId>),
    /// Deploy fetcher request.
    #[from]
    DeployFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, Deploy>),
    /// Block proposer request.
    #[from]
    BlockProposerRequest(#[serde(skip_serializing)] BlockProposerRequest),
    /// Block validator request.
    #[from]
    BlockValidatorRequest(#[serde(skip_serializing)] BlockValidationRequest<NodeId>),
    /// Metrics request.
    #[from]
    MetricsRequest(#[serde(skip_serializing)] MetricsRequest),
    /// Chainspec info request
    #[from]
    ChainspecLoaderRequest(#[serde(skip_serializing)] ChainspecLoaderRequest),
    /// Storage request.
    #[from]
    StorageRequest(#[serde(skip_serializing)] StorageRequest),
    /// Address gossip request.
    #[from]
    BeginAddressGossipRequest(BeginGossipRequest<GossipedAddress>),

    // Announcements
    /// Control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    /// API server announcement.
    #[from]
    RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
    /// DeployAcceptor announcement.
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement<NodeId>),
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
    BlocklistAnnouncement(BlocklistAnnouncement<NodeId>),
    /// Incoming consensus network message.
    #[from]
    ConsensusMessageIncoming(ConsensusMessageIncoming<NodeId>),
    /// Incoming deploy gossiper network message.
    #[from]
    DeployGossiperIncoming(GossiperIncoming<Deploy>),
    /// Incoming address gossiper network message.
    #[from]
    AddressGossiperIncoming(GossiperIncoming<GossipedAddress>),
    /// Incoming net request network message.
    #[from]
    NetRequestIncoming(NetRequestIncoming),
    /// Incoming net response network message.
    #[from]
    NetResponseIncoming(NetResponseIncoming),
    /// Incoming trie request network message.
    #[from]
    TrieRequestIncoming(TrieRequestIncoming),
    /// Incoming trie response network message.
    #[from]
    TrieResponseIncoming(TrieResponseIncoming),
    /// Incoming finality signature network message.
    #[from]
    FinalitySignatureIncoming(FinalitySignatureIncoming),
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
            ParticipatingEvent::NetworkRequest(_) => "NetworkRequest",
            ParticipatingEvent::NetworkInfoRequest(_) => "NetworkInfoRequest",
            ParticipatingEvent::DeployFetcherRequest(_) => "DeployFetcherRequest",
            ParticipatingEvent::BlockProposerRequest(_) => "BlockProposerRequest",
            ParticipatingEvent::BlockValidatorRequest(_) => "BlockValidatorRequest",
            ParticipatingEvent::MetricsRequest(_) => "MetricsRequest",
            ParticipatingEvent::ChainspecLoaderRequest(_) => "ChainspecLoaderRequest",
            ParticipatingEvent::StorageRequest(_) => "StorageRequest",
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

impl From<RpcRequest<NodeId>> for ParticipatingEvent {
    fn from(request: RpcRequest<NodeId>) -> Self {
        ParticipatingEvent::RpcServer(rpc_server::Event::RpcRequest(request))
    }
}

impl From<RestRequest<NodeId>> for ParticipatingEvent {
    fn from(request: RestRequest<NodeId>) -> Self {
        ParticipatingEvent::RestServer(rest_server::Event::RestRequest(request))
    }
}

impl From<NetworkRequest<NodeId, consensus::ConsensusMessage>> for ParticipatingEvent {
    fn from(request: NetworkRequest<NodeId, consensus::ConsensusMessage>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<Deploy>>> for ParticipatingEvent {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<Deploy>>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>> for ParticipatingEvent {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<ConsensusRequest> for ParticipatingEvent {
    fn from(request: ConsensusRequest) -> Self {
        ParticipatingEvent::Consensus(consensus::Event::ConsensusRequest(request))
    }
}

impl Display for ParticipatingEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ParticipatingEvent::Storage(event) => write!(f, "storage: {}", event),
            ParticipatingEvent::SmallNetwork(event) => write!(f, "small network: {}", event),
            ParticipatingEvent::BlockProposer(event) => write!(f, "block proposer: {}", event),
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
            ParticipatingEvent::NetworkRequest(req) => write!(f, "network request: {}", req),
            ParticipatingEvent::NetworkInfoRequest(req) => {
                write!(f, "network info request: {}", req)
            }
            ParticipatingEvent::ChainspecLoaderRequest(req) => {
                write!(f, "chainspec loader request: {}", req)
            }
            ParticipatingEvent::StorageRequest(req) => write!(f, "storage request: {}", req),

            ParticipatingEvent::DeployFetcherRequest(req) => {
                write!(f, "deploy fetcher request: {}", req)
            }
            ParticipatingEvent::BeginAddressGossipRequest(request) => {
                write!(f, "begin address gossip request: {}", request)
            }
            ParticipatingEvent::BlockProposerRequest(req) => {
                write!(f, "block proposer request: {}", req)
            }
            ParticipatingEvent::BlockValidatorRequest(req) => {
                write!(f, "block validator request: {}", req)
            }
            ParticipatingEvent::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            ParticipatingEvent::ControlAnnouncement(ctrl_ann) => write!(f, "control: {}", ctrl_ann),
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
            ParticipatingEvent::ConsensusMessageIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::DeployGossiperIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::AddressGossiperIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::NetRequestIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::NetResponseIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::TrieRequestIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::TrieResponseIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::FinalitySignatureIncoming(inner) => Display::fmt(inner, f),
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

    /// Inspect the contract runtime.
    pub(crate) fn contract_runtime(&self) -> &ContractRuntime {
        &self.contract_runtime
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
    consensus: EraSupervisor<NodeId>,
    #[data_size(skip)]
    deploy_acceptor: DeployAcceptor,
    deploy_fetcher: Fetcher<Deploy>,
    deploy_gossiper: Gossiper<Deploy, ParticipatingEvent>,
    block_proposer: BlockProposer,
    block_validator: BlockValidator<NodeId>,
    linear_chain: LinearChainComponent<NodeId>,

    // Non-components.
    #[data_size(skip)] // Never allocates heap data.
    memory_metrics: MemoryMetrics,

    #[data_size(skip)]
    event_queue_metrics: EventQueueMetrics,
}

#[cfg(test)]
impl Reactor {
    /// Inspect consensus.
    pub(crate) fn consensus(&self) -> &EraSupervisor<NodeId> {
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
        &self,
        effect_builder: EffectBuilder<<Self as reactor::Reactor>::Event>,
        sender: NodeId,
        tag: Tag,
        serialized_id: &[u8],
    ) -> Effects<<Self as reactor::Reactor>::Event> {
        match tag {
            Tag::Trie => {
                Self::respond_to_fetch(effect_builder, serialized_id, sender, |trie_key| {
                    self.contract_runtime.get_trie(trie_key)
                })
            }
            _ => todo!("already handled"),
        }
    }

    fn respond_to_fetch<T, F, E>(
        effect_builder: EffectBuilder<<Self as reactor::Reactor>::Event>,
        serialized_id: &[u8],
        sender: NodeId,
        fetch_item: F,
    ) -> Effects<<Self as reactor::Reactor>::Event>
    where
        T: Item,
        F: FnOnce(T::Id) -> Result<Option<T>, E>,
        E: Debug,
    {
        let id: T::Id = match bincode::deserialize(serialized_id) {
            Ok(id) => id,
            Err(error) => {
                error!(
                    tag = ?T::TAG,
                    ?serialized_id,
                    ?sender,
                    ?error,
                    "failed to decode item id"
                );
                return Effects::new();
            }
        };
        let fetched_or_not_found = match fetch_item(id) {
            Ok(Some(item)) => FetchedOrNotFound::Fetched(item),
            Ok(None) => {
                debug!(tag = ?T::TAG, ?id, ?sender, "failed to get item");
                FetchedOrNotFound::NotFound(id)
            }
            Err(error) => {
                error!(tag = ?T::TAG, ?id, ?sender, ?error, "error getting item");
                FetchedOrNotFound::NotFound(id)
            }
        };
        match Message::new_get_response(&fetched_or_not_found) {
            Ok(message) => effect_builder.send_message(sender, message).ignore(),
            Err(error) => {
                error!("failed to create get-response: {}", error);
                Effects::new()
            }
        }
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
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<ParticipatingEvent>), Error> {
        let ParticipatingInitConfig {
            root,
            config,
            chainspec_loader,
            mut storage,
            mut contract_runtime,
            maybe_latest_block_header,
            event_stream_server,
            small_network_identity,
            node_startup_instant,
        } = config;

        let memory_metrics = MemoryMetrics::new(registry.clone())?;

        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let metrics = Metrics::new(registry.clone());

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

        let maybe_next_activation_point = chainspec_loader
            .next_upgrade()
            .map(|next_upgrade| next_upgrade.activation_point());

        let mut effects = Effects::new();

        // If we have a latest block header, we joined via the joiner reactor.
        //
        // If not, run genesis or upgrade and construct a switch block, and use that for the latest
        // block header.
        let latest_block_header = if let Some(latest_block_header) = maybe_latest_block_header {
            latest_block_header
        } else {
            let initial_pre_state;
            let finalized_block;

            let chainspec = chainspec_loader.chainspec();
            match chainspec.protocol_config.activation_point {
                ActivationPoint::Genesis(genesis_timestamp) => {
                    // Do not run genesis on a node which is not protocol version 1.0.0
                    if chainspec.protocol_config.version != ProtocolVersion::V1_0_0 {
                        return Err(Error::GenesisNeedsProtocolVersion1_0_0 {
                            chainspec_protocol_version: chainspec.protocol_config.version,
                        });
                    }

                    // Check that no blocks exist in storage so we don't try to run genesis again on
                    // an existing blockchain.
                    if let Some(highest_block_header) = storage.read_highest_block_header()? {
                        return Err(Error::CannotRunGenesisOnPreExistingBlockchain {
                            highest_block_header: Box::new(highest_block_header),
                        });
                    }
                    let GenesisSuccess {
                        post_state_hash,
                        execution_effect,
                    } = contract_runtime.commit_genesis(chainspec)?;
                    info!("genesis chainspec name {}", chainspec.network_config.name);
                    info!("genesis state root hash {}", post_state_hash);
                    trace!(%post_state_hash, ?execution_effect);
                    initial_pre_state = ExecutionPreState::new(
                        0,
                        post_state_hash,
                        Default::default(),
                        Default::default(),
                    );
                    finalized_block = FinalizedBlock::new(
                        BlockPayload::default(),
                        Some(EraReport::default()),
                        genesis_timestamp,
                        EraId::from(0u64),
                        0,
                        PublicKey::System,
                    );
                }
                ActivationPoint::EraId(upgrade_era_id) => {
                    let upgrade_block_header = storage
                        .read_switch_block_header_by_era_id(upgrade_era_id.saturating_sub(1))?
                        .ok_or(Error::NoSuchSwitchBlockHeaderForUpgradeEra { upgrade_era_id })?;
                    // If it's not an emergency upgrade and there is a block higher than ours, bail
                    // because we will overwrite an existing blockchain.
                    if chainspec.protocol_config.last_emergency_restart
                        != Some(upgrade_block_header.era_id())
                    {
                        if let Some(preexisting_block_header) = storage
                            .read_block_header_by_height(upgrade_block_header.height() + 1)?
                        {
                            return Err(Error::NonEmergencyUpgradeWillClobberExistingBlockChain {
                                preexisting_block_header: Box::new(preexisting_block_header),
                            });
                        }
                    }
                    let global_state_update = chainspec.protocol_config.get_update_mapping()?;
                    let UpgradeSuccess {
                        post_state_hash,
                        execution_effect,
                    } = contract_runtime.commit_upgrade(UpgradeConfig::new(
                        *upgrade_block_header.state_root_hash(),
                        upgrade_block_header.protocol_version(),
                        chainspec.protocol_version(),
                        Some(chainspec.protocol_config.activation_point.era_id()),
                        Some(chainspec.core_config.validator_slots),
                        Some(chainspec.core_config.auction_delay),
                        Some(chainspec.core_config.locked_funds_period.millis()),
                        Some(chainspec.core_config.round_seigniorage_rate),
                        Some(chainspec.core_config.unbonding_delay),
                        global_state_update,
                    ))?;
                    info!(
                        network_name = %chainspec.network_config.name,
                        %post_state_hash,
                        "upgrade committed"
                    );
                    trace!(%post_state_hash, ?execution_effect);
                    initial_pre_state = ExecutionPreState::new(
                        upgrade_block_header.height() + 1,
                        post_state_hash,
                        upgrade_block_header.hash(),
                        upgrade_block_header.accumulated_seed(),
                    );
                    finalized_block = FinalizedBlock::new(
                        BlockPayload::default(),
                        Some(EraReport::default()),
                        upgrade_block_header.timestamp(),
                        upgrade_era_id,
                        upgrade_block_header.height() + 1,
                        PublicKey::System,
                    );
                }
            };
            // Execute the finalized block, creating a new switch block.
            let (new_switch_block, new_effects) = contract_runtime.execute_finalized_block(
                effect_builder,
                chainspec_loader.chainspec().protocol_version(),
                initial_pre_state,
                finalized_block,
                vec![],
                vec![],
            )?;
            // Make sure the new block really is a switch block
            if !new_switch_block.header().is_switch_block() {
                return Err(Error::FailedToCreateSwitchBlockAfterGenesisOrUpgrade {
                    new_bad_block: Box::new(new_switch_block),
                });
            }
            // Write the block to storage so the era supervisor can be initialized properly.
            storage.write_block(&new_switch_block)?;
            // Effects inform other components to make finality signatures, etc.
            effects.extend(reactor::wrap_effects(Into::into, new_effects));
            new_switch_block.take_header()
        };

        let (block_proposer, block_proposer_effects) = BlockProposer::new(
            registry.clone(),
            effect_builder,
            latest_block_header.height() + 1,
            chainspec_loader.chainspec().as_ref(),
            config.block_proposer,
        )?;
        effects.extend(reactor::wrap_effects(
            ParticipatingEvent::BlockProposer,
            block_proposer_effects,
        ));

        let (small_network, small_network_effects) = SmallNetwork::new(
            event_queue,
            config.network,
            Some(WithDir::new(&root, &config.consensus)),
            registry,
            small_network_identity,
            chainspec_loader.chainspec().as_ref(),
        )?;

        let (consensus, init_consensus_effects) = EraSupervisor::new(
            latest_block_header.next_block_era_id(),
            WithDir::new(root, config.consensus),
            effect_builder,
            chainspec_loader.chainspec().clone(),
            &latest_block_header,
            maybe_next_activation_point,
            registry,
            Box::new(HighwayProtocol::new_boxed),
            &storage,
            rng,
        )?;
        effects.extend(reactor::wrap_effects(
            ParticipatingEvent::Consensus,
            init_consensus_effects,
        ));

        contract_runtime.set_initial_state(ExecutionPreState::from(&latest_block_header));

        let block_validator = BlockValidator::new(Arc::clone(chainspec_loader.chainspec()));
        let linear_chain = linear_chain::LinearChainComponent::new(
            registry,
            *protocol_version,
            chainspec_loader.chainspec().core_config.auction_delay,
            chainspec_loader.chainspec().core_config.unbonding_delay,
            chainspec_loader
                .chainspec()
                .highway_config
                .finality_threshold_fraction,
            maybe_next_activation_point,
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
            ParticipatingEvent::Storage(event) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::SmallNetwork(event) => reactor::wrap_effects(
                ParticipatingEvent::SmallNetwork,
                self.small_network.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockProposer(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockProposer,
                self.block_proposer.handle_event(effect_builder, rng, event),
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
            ParticipatingEvent::StorageRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            ParticipatingEvent::BeginAddressGossipRequest(req) => reactor::wrap_effects(
                ParticipatingEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, req.into()),
            ),

            // Announcements:
            ParticipatingEvent::ControlAnnouncement(ctrl_ann) => {
                unreachable!("unhandled control announcement: {}", ctrl_ann)
            }
            ParticipatingEvent::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived {
                deploy,
                responder,
            }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::<NodeId>::Client,
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
                            .map(|(hash, (_header, results))| (*hash, results.clone()))
                            .collect(),
                    });
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));

                // send to event stream
                for (deploy_hash, (deploy_header, execution_result)) in execution_results {
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
                let reactor_event = ParticipatingEvent::LinearChain(
                    linear_chain::Event::GotUpgradeActivationPoint(next_upgrade.activation_point()),
                );
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                effects
            }
            ParticipatingEvent::BlocklistAnnouncement(ann) => self.dispatch_event(
                effect_builder,
                rng,
                ParticipatingEvent::SmallNetwork(ann.into()),
            ),
            ParticipatingEvent::ConsensusMessageIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::Consensus,
                self.consensus
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::DeployGossiperIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::AddressGossiperIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::NetRequestIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::Storage,
                self.storage
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            ParticipatingEvent::NetResponseIncoming(NetResponseIncoming { sender, message }) => {
                // TODO: Code to be refactored, we do not want to handle all this logic inside the
                //       routing function.
                let event = match message {
                    NetResponse::Deploy(ref serialized_item) => {
                        let deploy: Box<Deploy> = match bincode::deserialize::<
                            FetchedOrNotFound<Deploy, DeployHash>,
                        >(serialized_item)
                        {
                            Ok(FetchedOrNotFound::Fetched(deploy)) => Box::new(deploy),
                            Ok(FetchedOrNotFound::NotFound(deploy_hash)) => {
                                error!(
                                    "peer did not have deploy with hash {}: {}",
                                    sender, deploy_hash
                                );
                                return Effects::new();
                            }
                            Err(error) => {
                                error!("failed to decode deploy from {}: {}", sender, error);
                                return Effects::new();
                            }
                        };

                        ParticipatingEvent::DeployAcceptor(deploy_acceptor::Event::Accept {
                            deploy,
                            source: Source::Peer(sender),
                            responder: None,
                        })
                    }
                    NetResponse::Block(_) => {
                        error!(
                            "cannot handle get response for block-by-hash from {}",
                            sender
                        );
                        return Effects::new();
                    }
                    NetResponse::GossipedAddress(_) => {
                        error!(
                            "cannot handle get response for gossiped-address from {}",
                            sender
                        );
                        return Effects::new();
                    }
                    NetResponse::BlockAndMetadataByHeight(_) => {
                        error!(
                            "cannot handle get response for block-by-height from {}",
                            sender
                        );
                        return Effects::new();
                    }
                    NetResponse::BlockHeaderByHash(_) => {
                        error!(
                            "cannot handle get response for block-header-by-hash from {}",
                            sender
                        );
                        return Effects::new();
                    }
                    NetResponse::BlockHeaderAndFinalitySignaturesByHeight(_) => {
                        error!(
                            "cannot handle get response for \
                            block-header-and-finality-signatures-by-height from {}",
                            sender
                        );
                        return Effects::new();
                    }
                };

                self.dispatch_event(effect_builder, rng, event)
            }
            ParticipatingEvent::TrieRequestIncoming(TrieRequestIncoming {
                sender,
                message: TrieRequest(ref serialized_id),
            }) => self.handle_get_request(effect_builder, sender, Tag::Trie, serialized_id),
            ParticipatingEvent::TrieResponseIncoming(TrieResponseIncoming { sender, .. }) => {
                error!("cannot handle get response for read-trie from {}", sender);
                return Effects::new();
            }
            ParticipatingEvent::FinalitySignatureIncoming(incoming) => reactor::wrap_effects(
                ParticipatingEvent::LinearChain,
                self.linear_chain
                    .handle_event(effect_builder, rng, incoming.into()),
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
        self.linear_chain
            .stop_for_upgrade()
            .then(|| ReactorExit::ProcessShouldExit(ExitCode::Success))
    }
}

#[cfg(test)]
impl NetworkedReactor for Reactor {
    type NodeId = NodeId;
    fn node_id(&self) -> Self::NodeId {
        self.small_network.node_id()
    }
}
