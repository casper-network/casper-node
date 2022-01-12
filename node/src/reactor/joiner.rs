//! Reactor used to join the network.

mod memory_metrics;

use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    path::PathBuf,
    time::Instant,
};

use datasize::DataSize;
use derive_more::From;
use memory_metrics::MemoryMetrics;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tracing::{debug, error, info, warn};

use casper_execution_engine::storage::trie::TrieOrChunk;

#[cfg(test)]
use crate::testing::network::NetworkedReactor;
use crate::{
    components::{
        chainspec_loader::{self, ChainspecLoader},
        contract_runtime::ContractRuntime,
        deploy_acceptor::{self, DeployAcceptor},
        event_stream_server,
        event_stream_server::{DeployGetter, EventStreamServer},
        fetcher::{self, Fetcher, TrieFetcher, TrieFetcherEvent},
        gossiper::{self, Gossiper},
        linear_chain_sync::{self, LinearChainSyncError, LinearChainSyncState},
        metrics::Metrics,
        rest_server::{self, RestServer},
        small_network::{self, GossipedAddress, SmallNetwork, SmallNetworkIdentity},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            BlocklistAnnouncement, ChainspecLoaderAnnouncement, ContractRuntimeAnnouncement,
            ControlAnnouncement, DeployAcceptorAnnouncement, GossiperAnnouncement,
            LinearChainAnnouncement,
        },
        incoming::{
            ConsensusMessageIncoming, FinalitySignatureIncoming, GossiperIncoming,
            NetRequestIncoming, NetResponse, NetResponseIncoming, TrieRequestIncoming,
            TrieResponseIncoming,
        },
        requests::{
            BeginGossipRequest, ChainspecLoaderRequest, ConsensusRequest, ContractRuntimeRequest,
            FetcherRequest, MetricsRequest, NetworkInfoRequest, NetworkRequest, RestRequest,
            StorageRequest, TrieFetcherRequest,
        },
        EffectBuilder, EffectExt, EffectOptionExt, Effects,
    },
    fatal,
    protocol::Message,
    reactor::{
        self,
        event_queue_metrics::EventQueueMetrics,
        initializer,
        participating::{self, Error, ParticipatingInitConfig},
        EventQueueHandle, Finalize, ReactorExit,
    },
    types::{
        Block, BlockHeader, BlockHeaderWithMetadata, BlockWithMetadata, Deploy, ExitCode, NodeId,
        Timestamp,
    },
    utils::WithDir,
    NodeRng,
};

/// Top-level event for the reactor.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, From, Serialize)]
#[must_use]
pub(crate) enum JoinerEvent {
    /// Finished joining event.
    FinishedJoining { block_header: Box<BlockHeader> },

    /// Shut down with the given exit code.
    Shutdown(#[serde(skip_serializing)] ExitCode),

    /// Small Network event.
    #[from]
    SmallNetwork(small_network::Event<Message>),

    /// Storage event.
    #[from]
    Storage(storage::Event),

    #[from]
    /// REST server event.
    RestServer(#[serde(skip_serializing)] rest_server::Event),

    #[from]
    /// Event stream server event.
    EventStreamServer(#[serde(skip_serializing)] event_stream_server::Event),

    /// Metrics request.
    #[from]
    MetricsRequest(#[serde(skip_serializing)] MetricsRequest),

    #[from]
    /// Chainspec Loader event.
    ChainspecLoader(#[serde(skip_serializing)] chainspec_loader::Event),

    /// Chainspec info request
    #[from]
    ChainspecLoaderRequest(#[serde(skip_serializing)] ChainspecLoaderRequest),

    /// Network info request.
    #[from]
    NetworkInfoRequest(#[serde(skip_serializing)] NetworkInfoRequest<NodeId>),

    /// Block fetcher event.
    #[from]
    BlockFetcher(#[serde(skip_serializing)] fetcher::Event<Block>),

    /// Block header (without metadata) fetcher event.
    #[from]
    BlockHeaderFetcher(#[serde(skip_serializing)] fetcher::Event<BlockHeader>),

    /// Block header with metadata by height fetcher event.
    #[from]
    BlockHeaderByHeightFetcher(#[serde(skip_serializing)] fetcher::Event<BlockHeaderWithMetadata>),

    /// Linear chain (by height) fetcher event.
    #[from]
    BlockByHeightFetcher(#[serde(skip_serializing)] fetcher::Event<BlockWithMetadata>),

    /// Deploy fetcher event.
    #[from]
    DeployFetcher(#[serde(skip_serializing)] fetcher::Event<Deploy>),

    /// Trie or chunk fetcher event.
    #[from]
    TrieOrChunkFetcher(#[serde(skip_serializing)] fetcher::Event<TrieOrChunk>),

    /// Trie fetcher event.
    #[from]
    TrieFetcher(#[serde(skip_serializing)] TrieFetcherEvent<NodeId>),

    /// Deploy acceptor event.
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] deploy_acceptor::Event),

    /// Contract Runtime event.
    #[from]
    ContractRuntime(#[serde(skip_serializing)] ContractRuntimeRequest),

    /// Address gossiper event.
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),

    // Requests.
    /// Storage request.
    #[from]
    StorageRequest(StorageRequest),

    /// Linear chain block by hash fetcher request.
    #[from]
    BlockFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, Block>),

    /// Blocker header (with no metadata) fetcher request.
    #[from]
    BlockHeaderFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, BlockHeader>),

    /// Trie or chunk fetcher request.
    #[from]
    TrieOrChunkFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, TrieOrChunk>),

    /// Trie or chunk fetcher request.
    #[from]
    TrieFetcherRequest(#[serde(skip_serializing)] TrieFetcherRequest<NodeId>),

    /// Block header with metadata by height fetcher request.
    #[from]
    BlockHeaderByHeightFetcherRequest(
        #[serde(skip_serializing)] FetcherRequest<NodeId, BlockHeaderWithMetadata>,
    ),

    /// Linear chain block by height fetcher request.
    #[from]
    BlockByHeightFetcherRequest(
        #[serde(skip_serializing)] FetcherRequest<NodeId, BlockWithMetadata>,
    ),

    /// Deploy fetcher request.
    #[from]
    DeployFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, Deploy>),

    /// Address gossip request.
    #[from]
    BeginAddressGossipRequest(BeginGossipRequest<GossipedAddress>),

    // Announcements
    /// A control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),

    /// Blocklist announcement.
    #[from]
    BlocklistAnnouncement(#[serde(skip_serializing)] BlocklistAnnouncement<NodeId>),

    /// Block executor announcement.
    #[from]
    ContractRuntimeAnnouncement(#[serde(skip_serializing)] ContractRuntimeAnnouncement),

    /// Address Gossiper announcement.
    #[from]
    AddressGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<GossipedAddress>),

    /// DeployAcceptor announcement.
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement<NodeId>),

    /// Linear chain announcement.
    #[from]
    LinearChainAnnouncement(#[serde(skip_serializing)] LinearChainAnnouncement),

    /// Chainspec loader announcement.
    #[from]
    ChainspecLoaderAnnouncement(#[serde(skip_serializing)] ChainspecLoaderAnnouncement),

    /// Consensus request.
    #[from]
    ConsensusRequest(#[serde(skip_serializing)] ConsensusRequest),

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
}

impl ReactorEvent for JoinerEvent {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
    fn description(&self) -> &'static str {
        match self {
            JoinerEvent::SmallNetwork(_) => "SmallNetwork",
            JoinerEvent::Storage(_) => "Storage",
            JoinerEvent::RestServer(_) => "RestServer",
            JoinerEvent::EventStreamServer(_) => "EventStreamServer",
            JoinerEvent::MetricsRequest(_) => "MetricsRequest",
            JoinerEvent::ChainspecLoader(_) => "ChainspecLoader",
            JoinerEvent::ChainspecLoaderRequest(_) => "ChainspecLoaderRequest",
            JoinerEvent::NetworkInfoRequest(_) => "NetworkInfoRequest",
            JoinerEvent::BlockFetcher(_) => "BlockFetcher",
            JoinerEvent::BlockByHeightFetcher(_) => "BlockByHeightFetcher",
            JoinerEvent::DeployFetcher(_) => "DeployFetcher",
            JoinerEvent::TrieOrChunkFetcher(_) => "TrieOrChunkFetcher",
            JoinerEvent::TrieFetcher(_) => "TrieFetcher",
            JoinerEvent::DeployAcceptor(_) => "DeployAcceptor",
            JoinerEvent::ContractRuntime(_) => "ContractRuntime",
            JoinerEvent::AddressGossiper(_) => "AddressGossiper",
            JoinerEvent::BlockFetcherRequest(_) => "BlockFetcherRequest",
            JoinerEvent::BlockByHeightFetcherRequest(_) => "BlockByHeightFetcherRequest",
            JoinerEvent::DeployFetcherRequest(_) => "DeployFetcherRequest",
            JoinerEvent::TrieOrChunkFetcherRequest(_) => "TrieOrChunkFetcherRequest",
            JoinerEvent::TrieFetcherRequest(_) => "TrieFetcherRequest",
            JoinerEvent::ControlAnnouncement(_) => "ControlAnnouncement",
            JoinerEvent::ContractRuntimeAnnouncement(_) => "ContractRuntimeAnnouncement",
            JoinerEvent::AddressGossiperAnnouncement(_) => "AddressGossiperAnnouncement",
            JoinerEvent::DeployAcceptorAnnouncement(_) => "DeployAcceptorAnnouncement",
            JoinerEvent::LinearChainAnnouncement(_) => "LinearChainAnnouncement",
            JoinerEvent::ChainspecLoaderAnnouncement(_) => "ChainspecLoaderAnnouncement",
            JoinerEvent::ConsensusRequest(_) => "ConsensusRequest",
            JoinerEvent::BlockHeaderFetcher(_) => "BlockHeaderFetcher",
            JoinerEvent::BlockHeaderByHeightFetcher(_) => "BlockHeaderByHeightFetcher",
            JoinerEvent::BlockHeaderFetcherRequest(_) => "BlockHeaderFetcherRequest",
            JoinerEvent::BlockHeaderByHeightFetcherRequest(_) => {
                "BlockHeaderByHeightFetcherRequest"
            }
            JoinerEvent::FinishedJoining { .. } => "FinishedJoining",
            JoinerEvent::Shutdown(_) => "Shutdown",
            JoinerEvent::BlocklistAnnouncement(_) => "BlocklistAnnouncement",
            JoinerEvent::StorageRequest(_) => "StorageRequest",
            JoinerEvent::BeginAddressGossipRequest(_) => "BeginAddressGossipRequest",
            JoinerEvent::ConsensusMessageIncoming(_) => "ConsensusMessageIncoming",
            JoinerEvent::DeployGossiperIncoming(_) => "DeployGossiperIncoming",
            JoinerEvent::AddressGossiperIncoming(_) => "AddressGossiperIncoming",
            JoinerEvent::NetRequestIncoming(_) => "NetRequestIncoming",
            JoinerEvent::NetResponseIncoming(_) => "NetResponseIncoming",
            JoinerEvent::TrieRequestIncoming(_) => "TrieRequestIncoming",
            JoinerEvent::TrieResponseIncoming(_) => "TrieResponseIncoming",
            JoinerEvent::FinalitySignatureIncoming(_) => "FinalitySignatureIncoming",
        }
    }
}

impl From<NetworkRequest<NodeId, Message>> for JoinerEvent {
    fn from(request: NetworkRequest<NodeId, Message>) -> Self {
        JoinerEvent::SmallNetwork(small_network::Event::from(request))
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>> for JoinerEvent {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>) -> Self {
        JoinerEvent::SmallNetwork(small_network::Event::from(
            request.map_payload(Message::from),
        ))
    }
}

impl From<RestRequest<NodeId>> for JoinerEvent {
    fn from(request: RestRequest<NodeId>) -> Self {
        JoinerEvent::RestServer(rest_server::Event::RestRequest(request))
    }
}

impl Display for JoinerEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            JoinerEvent::SmallNetwork(event) => write!(f, "small network: {}", event),
            JoinerEvent::BlocklistAnnouncement(event) => {
                write!(f, "blocklist announcement: {}", event)
            }
            JoinerEvent::Storage(request) => write!(f, "storage: {}", request),
            JoinerEvent::RestServer(event) => write!(f, "rest server: {}", event),
            JoinerEvent::EventStreamServer(event) => write!(f, "event stream server: {}", event),
            JoinerEvent::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            JoinerEvent::ChainspecLoader(event) => write!(f, "chainspec loader: {}", event),
            JoinerEvent::ChainspecLoaderRequest(req) => {
                write!(f, "chainspec loader request: {}", req)
            }
            JoinerEvent::StorageRequest(req) => write!(f, "storage request: {}", req),
            JoinerEvent::NetworkInfoRequest(req) => write!(f, "network info request: {}", req),
            JoinerEvent::BlockFetcherRequest(request) => {
                write!(f, "block fetcher request: {}", request)
            }
            JoinerEvent::DeployFetcherRequest(request) => {
                write!(f, "deploy fetcher request: {}", request)
            }
            JoinerEvent::BeginAddressGossipRequest(request) => {
                write!(f, "begin address gossip request: {}", request)
            }
            JoinerEvent::TrieOrChunkFetcherRequest(request) => {
                write!(f, "trie or chunk fetcher request: {}", request)
            }
            JoinerEvent::TrieFetcherRequest(request) => {
                write!(f, "trie fetcher request: {}", request)
            }
            JoinerEvent::BlockFetcher(event) => write!(f, "block fetcher: {}", event),
            JoinerEvent::BlockByHeightFetcherRequest(request) => {
                write!(f, "block by height fetcher request: {}", request)
            }
            JoinerEvent::TrieOrChunkFetcher(event) => {
                write!(f, "trie or chunk fetcher: {}", event)
            }
            JoinerEvent::TrieFetcher(event) => write!(f, "trie fetcher: {}", event),
            JoinerEvent::DeployFetcher(event) => write!(f, "deploy fetcher event: {}", event),
            JoinerEvent::ContractRuntime(event) => write!(f, "contract runtime event: {:?}", event),
            JoinerEvent::ContractRuntimeAnnouncement(announcement) => {
                write!(f, "block executor announcement: {}", announcement)
            }
            JoinerEvent::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            JoinerEvent::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
            JoinerEvent::BlockByHeightFetcher(event) => {
                write!(f, "block by height fetcher event: {}", event)
            }
            JoinerEvent::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            JoinerEvent::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            JoinerEvent::ControlAnnouncement(ctrl_ann) => write!(f, "control: {}", ctrl_ann),
            JoinerEvent::LinearChainAnnouncement(ann) => {
                write!(f, "linear chain announcement: {}", ann)
            }
            JoinerEvent::ChainspecLoaderAnnouncement(ann) => {
                write!(f, "chainspec loader announcement: {}", ann)
            }
            JoinerEvent::ConsensusRequest(req) => write!(f, "consensus request: {:?}", req),
            JoinerEvent::BlockHeaderFetcher(block_header) => {
                write!(f, "block header fetcher event: {}", block_header)
            }
            JoinerEvent::BlockHeaderFetcherRequest(req) => {
                write!(f, "block header fetcher request: {}", req)
            }
            JoinerEvent::BlockHeaderByHeightFetcher(block_header_by_height) => {
                write!(
                    f,
                    "block header by height fetcher event: {}",
                    block_header_by_height
                )
            }
            JoinerEvent::BlockHeaderByHeightFetcherRequest(req) => {
                write!(f, "block header by height fetcher request: {}", req)
            }
            JoinerEvent::FinishedJoining { block_header } => {
                write!(f, "finished joining with block header: {}", block_header)
            }
            JoinerEvent::ConsensusMessageIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::DeployGossiperIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::AddressGossiperIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::NetRequestIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::NetResponseIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::TrieRequestIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::TrieResponseIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::FinalitySignatureIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::Shutdown(exit_code) => {
                write!(f, "shutting down with exit code: {:?}", exit_code)
            }
        }
    }
}

/// Joining node reactor.
#[derive(DataSize)]
pub(crate) struct Reactor {
    root: PathBuf,
    metrics: Metrics,
    small_network: SmallNetwork<JoinerEvent, Message>,
    address_gossiper: Gossiper<GossipedAddress, JoinerEvent>,
    config: participating::Config,
    chainspec_loader: ChainspecLoader,
    storage: Storage,
    contract_runtime: ContractRuntime,
    linear_chain_sync: LinearChainSyncState,
    deploy_fetcher: Fetcher<Deploy>,
    block_by_hash_fetcher: Fetcher<Block>,
    block_by_height_fetcher: Fetcher<BlockWithMetadata>,
    block_header_by_hash_fetcher: Fetcher<BlockHeader>,
    block_header_and_finality_signatures_by_height_fetcher: Fetcher<BlockHeaderWithMetadata>,
    /// This is set to `Some` if the node should shut down.
    exit_code: Option<ExitCode>,
    trie_or_chunk_fetcher: Fetcher<TrieOrChunk>,
    // Handles requests for fetching tries from the network.
    trie_fetcher: TrieFetcher<NodeId>,
    #[data_size(skip)]
    deploy_acceptor: DeployAcceptor,
    #[data_size(skip)]
    event_queue_metrics: EventQueueMetrics,
    #[data_size(skip)]
    rest_server: RestServer,
    #[data_size(skip)]
    event_stream_server: EventStreamServer,
    // Attach memory metrics for the joiner.
    #[data_size(skip)] // Never allocates data on the heap.
    memory_metrics: MemoryMetrics,
    node_startup_instant: Instant,
}

impl reactor::Reactor for Reactor {
    type Event = JoinerEvent;

    // The "configuration" is in fact the whole state of the initializer reactor, which we
    // deconstruct and reuse.
    type Config = WithDir<initializer::Reactor>;
    type Error = Error;

    fn new(
        initializer: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (root, initializer) = initializer.into_parts();

        let initializer::Reactor {
            config: with_dir_config,
            chainspec_loader,
            storage,
            contract_runtime,
            small_network_identity,
        } = initializer;

        // We don't need to be super precise about the startup time, i.e.
        // we can skip the time spent in `initializer` for the sake of code simplicity.
        let node_startup_instant = Instant::now();
        // TODO: Remove wrapper around Reactor::Config instead.
        let (_, config) = with_dir_config.into_parts();

        let memory_metrics = MemoryMetrics::new(registry.clone())?;

        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let metrics = Metrics::new(registry.clone());

        let (small_network, small_network_effects) = SmallNetwork::new(
            event_queue,
            config.network.clone(),
            Some(WithDir::new(&root, &config.consensus)),
            registry,
            small_network_identity,
            chainspec_loader.chainspec().as_ref(),
        )?;

        let mut effects = reactor::wrap_effects(JoinerEvent::SmallNetwork, small_network_effects);

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let effect_builder = EffectBuilder::new(event_queue);

        let maybe_trusted_hash = match config.node.trusted_hash {
            Some(trusted_hash) => Some(trusted_hash),
            None => storage
                .read_highest_block_header()
                .expect("Could not read highest block header")
                .map(|block_header| block_header.hash()),
        };

        let chainspec = chainspec_loader.chainspec().clone();
        let linear_chain_sync = match maybe_trusted_hash {
            None => {
                let genesis_timestamp = chainspec
                    .protocol_config
                    .activation_point
                    .genesis_timestamp();
                let era_duration = chainspec.core_config.era_duration;
                let genesis_era_ended = genesis_timestamp.map_or(true, |start_time| {
                    Timestamp::now() > start_time + era_duration
                });
                if genesis_era_ended {
                    error!(
                            now=?Timestamp::now(),
                            genesis_era_end=?genesis_timestamp
                                .map(|start_time| start_time + era_duration),
                            "node started with no trusted hash after the expected end of \
                             the genesis era! Please specify a trusted hash and restart.");
                    panic!("should have trusted hash after genesis era")
                }
                LinearChainSyncState::NotGoingToSync
            }
            Some(hash) => {
                let node_config = config.node.clone();
                effects.extend(
                    (async move {
                        info!(trusted_hash=%hash, "synchronizing linear chain");
                        generate_joiner_event(
                            linear_chain_sync::run_fast_sync_task(
                                effect_builder,
                                hash,
                                chainspec,
                                node_config,
                            )
                            .await,
                            effect_builder,
                        )
                        .await
                    })
                    .map_some(std::convert::identity),
                );
                LinearChainSyncState::Syncing
            }
        };

        let protocol_version = &chainspec_loader.chainspec().protocol_config.version;
        let rest_server = RestServer::new(
            config.rest_server.clone(),
            effect_builder,
            *protocol_version,
            node_startup_instant,
        )?;

        let event_stream_server = EventStreamServer::new(
            config.event_stream_server.clone(),
            storage.root_path().to_path_buf(),
            *protocol_version,
            DeployGetter::new(effect_builder),
        )?;

        let deploy_fetcher = Fetcher::new("deploy", config.fetcher, registry)?;

        let block_by_height_fetcher = Fetcher::new("block_by_height", config.fetcher, registry)?;

        let block_by_hash_fetcher = Fetcher::new("block", config.fetcher, registry)?;
        let block_header_and_finality_signatures_by_height_fetcher =
            Fetcher::new("block_header_by_height", config.fetcher, registry)?;

        let block_header_by_hash_fetcher: Fetcher<BlockHeader> =
            Fetcher::new("block_header", config.fetcher, registry)?;

        let trie_or_chunk_fetcher = Fetcher::new("trie_or_chunk", config.fetcher, registry)?;
        let trie_fetcher = TrieFetcher::new();

        let deploy_acceptor = DeployAcceptor::new(
            config.deploy_acceptor,
            &*chainspec_loader.chainspec(),
            registry,
        )?;

        effects.extend(reactor::wrap_effects(
            JoinerEvent::ChainspecLoader,
            chainspec_loader.start_checking_for_upgrades(effect_builder),
        ));

        Ok((
            Self {
                root,
                metrics,
                small_network,
                address_gossiper,
                config,
                chainspec_loader,
                storage,
                contract_runtime,
                linear_chain_sync,
                block_by_hash_fetcher,
                deploy_fetcher,
                block_by_height_fetcher,
                block_header_by_hash_fetcher,
                block_header_and_finality_signatures_by_height_fetcher,
                exit_code: None,
                trie_or_chunk_fetcher,
                trie_fetcher,
                deploy_acceptor,
                event_queue_metrics,
                rest_server,
                event_stream_server,
                memory_metrics,
                node_startup_instant,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            JoinerEvent::SmallNetwork(event) => reactor::wrap_effects(
                JoinerEvent::SmallNetwork,
                self.small_network.handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::ControlAnnouncement(ctrl_ann) => {
                unreachable!("unhandled control announcement: {}", ctrl_ann)
            }
            JoinerEvent::BlocklistAnnouncement(ann) => {
                self.dispatch_event(effect_builder, rng, JoinerEvent::SmallNetwork(ann.into()))
            }
            JoinerEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            ) => {
                let event = event_stream_server::Event::DeployAccepted(*deploy.id());
                let mut effects =
                    self.dispatch_event(effect_builder, rng, JoinerEvent::EventStreamServer(event));

                let event = fetcher::Event::GotRemotely {
                    item: deploy,
                    source,
                };
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    JoinerEvent::DeployFetcher(event),
                ));

                effects
            }
            JoinerEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::InvalidDeploy { deploy, source },
            ) => {
                let deploy_hash = *deploy.id();
                let peer = source;
                warn!(?deploy_hash, ?peer, "Invalid deploy received from a peer.");
                Effects::new()
            }
            JoinerEvent::Storage(event) => reactor::wrap_effects(
                JoinerEvent::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::BlockFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::BlockFetcher(request.into()),
            ),
            JoinerEvent::DeployAcceptor(event) => reactor::wrap_effects(
                JoinerEvent::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::BlockFetcher(event) => reactor::wrap_effects(
                JoinerEvent::BlockFetcher,
                self.block_by_hash_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::DeployFetcher(event) => reactor::wrap_effects(
                JoinerEvent::DeployFetcher,
                self.deploy_fetcher.handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::BlockByHeightFetcher(event) => reactor::wrap_effects(
                JoinerEvent::BlockByHeightFetcher,
                self.block_by_height_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::BlockHeaderFetcher(event) => reactor::wrap_effects(
                JoinerEvent::BlockHeaderFetcher,
                self.block_header_by_hash_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::DeployFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::DeployFetcher(request.into()),
            ),
            JoinerEvent::StorageRequest(req) => reactor::wrap_effects(
                JoinerEvent::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            JoinerEvent::BeginAddressGossipRequest(req) => reactor::wrap_effects(
                JoinerEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, req.into()),
            ),
            JoinerEvent::BlockByHeightFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::BlockByHeightFetcher(request.into()),
            ),
            JoinerEvent::BlockHeaderFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::BlockHeaderFetcher(request.into()),
            ),
            JoinerEvent::TrieOrChunkFetcher(event) => reactor::wrap_effects(
                JoinerEvent::TrieOrChunkFetcher,
                self.trie_or_chunk_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::TrieFetcher(event) => reactor::wrap_effects(
                JoinerEvent::TrieFetcher,
                self.trie_fetcher.handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::ContractRuntime(event) => reactor::wrap_effects(
                JoinerEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::ContractRuntimeAnnouncement(_) => Effects::new(),
            JoinerEvent::AddressGossiper(event) => reactor::wrap_effects(
                JoinerEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::AddressGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_address,
            )) => {
                let reactor_event = JoinerEvent::SmallNetwork(
                    small_network::Event::PeerAddressReceived(gossiped_address),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            JoinerEvent::AddressGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                _,
            )) => {
                // We don't care about completion of gossiping an address.
                Effects::new()
            }

            JoinerEvent::LinearChainAnnouncement(LinearChainAnnouncement::BlockAdded(block)) => {
                reactor::wrap_effects(
                    JoinerEvent::EventStreamServer,
                    self.event_stream_server.handle_event(
                        effect_builder,
                        rng,
                        event_stream_server::Event::BlockAdded(block),
                    ),
                )
            }
            JoinerEvent::LinearChainAnnouncement(
                LinearChainAnnouncement::NewFinalitySignature(fs),
            ) => {
                let reactor_event = JoinerEvent::EventStreamServer(
                    event_stream_server::Event::FinalitySignature(fs),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            JoinerEvent::RestServer(event) => reactor::wrap_effects(
                JoinerEvent::RestServer,
                self.rest_server.handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::EventStreamServer(event) => reactor::wrap_effects(
                JoinerEvent::EventStreamServer,
                self.event_stream_server
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::MetricsRequest(req) => reactor::wrap_effects(
                JoinerEvent::MetricsRequest,
                self.metrics.handle_event(effect_builder, rng, req),
            ),
            JoinerEvent::ChainspecLoader(event) => reactor::wrap_effects(
                JoinerEvent::ChainspecLoader,
                self.chainspec_loader
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::ChainspecLoaderRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::ChainspecLoader(req.into()),
            ),
            JoinerEvent::NetworkInfoRequest(req) => {
                let event = JoinerEvent::SmallNetwork(small_network::Event::from(req));
                self.dispatch_event(effect_builder, rng, event)
            }
            JoinerEvent::ChainspecLoaderAnnouncement(
                ChainspecLoaderAnnouncement::UpgradeActivationPointRead(next_upgrade),
            ) => {
                let reactor_event = JoinerEvent::ChainspecLoader(
                    chainspec_loader::Event::GotNextUpgrade(next_upgrade),
                );
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            // This is done to handle status requests from the RestServer
            JoinerEvent::ConsensusRequest(ConsensusRequest::Status(responder)) => {
                // no consensus, respond with None
                responder.respond(None).ignore()
            }
            JoinerEvent::ConsensusRequest(ConsensusRequest::ValidatorChanges(responder)) => {
                // no consensus, respond with empty map
                responder.respond(BTreeMap::new()).ignore()
            }
            JoinerEvent::BlockHeaderByHeightFetcher(event) => reactor::wrap_effects(
                JoinerEvent::BlockHeaderByHeightFetcher,
                self.block_header_and_finality_signatures_by_height_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::BlockHeaderByHeightFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::BlockHeaderByHeightFetcher(request.into()),
            ),
            JoinerEvent::TrieOrChunkFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::TrieOrChunkFetcher(request.into()),
            ),
            JoinerEvent::TrieFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::TrieFetcher(request.into()),
            ),
            JoinerEvent::FinishedJoining { block_header } => {
                self.linear_chain_sync = LinearChainSyncState::Done(block_header);
                Effects::new()
            }
            JoinerEvent::ConsensusMessageIncoming(incoming) => {
                debug!(%incoming, "ignoring incoming consensus message");
                Effects::new()
            }
            JoinerEvent::DeployGossiperIncoming(incoming) => {
                debug!(%incoming, "ignoring incoming deploy gossiper message");
                Effects::new()
            }
            JoinerEvent::AddressGossiperIncoming(incoming) => reactor::wrap_effects(
                JoinerEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            JoinerEvent::NetRequestIncoming(incoming) => {
                debug!(%incoming, "net request ignored");
                Effects::new()
            }
            JoinerEvent::NetResponseIncoming(NetResponseIncoming { sender, message }) => {
                self.handle_get_response(effect_builder, rng, sender, message)
            }
            JoinerEvent::TrieRequestIncoming(incoming) => {
                debug!(%incoming, "trie request ignored");
                Effects::new()
            }
            JoinerEvent::TrieResponseIncoming(TrieResponseIncoming { sender, message }) => {
                reactor::handle_fetch_response::<Self, TrieOrChunk>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    &message.0,
                )
            }

            JoinerEvent::FinalitySignatureIncoming(FinalitySignatureIncoming {
                sender, ..
            }) => {
                debug!(%sender, "finality signatures not handled in joiner reactor");
                Effects::new()
            }
            JoinerEvent::Shutdown(exit_code) => {
                self.exit_code = Some(exit_code);
                Effects::new()
            }
        }
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        if let Some(exit_code) = self.exit_code {
            Some(ReactorExit::ProcessShouldExit(exit_code))
        } else if self.linear_chain_sync.is_synced() {
            Some(ReactorExit::ProcessShouldContinue)
        } else {
            None
        }
    }

    fn update_metrics(&mut self, event_queue_handle: EventQueueHandle<Self::Event>) {
        self.memory_metrics.estimate(self);
        self.event_queue_metrics
            .record_event_queue_counts(&event_queue_handle);
    }
}

async fn generate_joiner_event(
    fast_sync_task_result: Result<BlockHeader, LinearChainSyncError>,
    effect_builder: EffectBuilder<JoinerEvent>,
) -> Option<JoinerEvent> {
    match fast_sync_task_result {
        Ok(block_header) => Some(JoinerEvent::FinishedJoining {
            block_header: Box::new(block_header),
        }),
        Err(LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
            current_version,
            block_header_with_future_version,
        }) => {
            let future_version = block_header_with_future_version.protocol_version();
            info!(%current_version, %future_version, "restarting for upgrade");
            Some(JoinerEvent::Shutdown(ExitCode::Success))
        }
        Err(error) => {
            fatal!(effect_builder, "{}", error).await;
            None
        }
    }
}

impl Reactor {
    fn handle_get_response(
        &mut self,
        effect_builder: EffectBuilder<JoinerEvent>,
        rng: &mut NodeRng,
        sender: NodeId,
        message: NetResponse,
    ) -> Effects<JoinerEvent> {
        match message {
            NetResponse::Deploy(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, Deploy>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                )
            }
            NetResponse::Block(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, Block>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                )
            }
            NetResponse::GossipedAddress(_) => {
                // The item trait is used for both fetchers and gossiped things, but this kind of
                // item is never fetched, only gossiped.
                warn!(
                    "Gossiped addresses are never fetched, banning peer: {}",
                    sender
                );
                effect_builder
                    .announce_disconnect_from_peer(sender)
                    .ignore()
            }
            NetResponse::BlockAndMetadataByHeight(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, BlockWithMetadata>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                )
            }
            NetResponse::BlockHeaderByHash(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, BlockHeader>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                )
            }
            NetResponse::BlockHeaderAndFinalitySignaturesByHeight(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, BlockHeaderWithMetadata>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                )
            }
        }
    }

    /// Deconstructs the reactor into config useful for creating a Validator reactor. Shuts down
    /// the network, closing all incoming and outgoing connections, and frees up the listening
    /// socket.
    pub(crate) async fn into_participating_config(self) -> Result<ParticipatingInitConfig, Error> {
        let maybe_latest_block_header = self.linear_chain_sync.into_maybe_latest_block_header();
        let config = ParticipatingInitConfig {
            root: self.root,
            chainspec_loader: self.chainspec_loader,
            config: self.config,
            contract_runtime: self.contract_runtime,
            storage: self.storage,
            maybe_latest_block_header,
            event_stream_server: self.event_stream_server,
            small_network_identity: SmallNetworkIdentity::from(&self.small_network),
            node_startup_instant: self.node_startup_instant,
        };
        self.small_network.finalize().await;
        self.rest_server.finalize().await;
        Ok(config)
    }
}

#[cfg(test)]
impl NetworkedReactor for Reactor {
    type NodeId = NodeId;
    fn node_id(&self) -> Self::NodeId {
        self.small_network.node_id()
    }
}

#[cfg(test)]
impl Reactor {
    /// Inspect storage.
    pub(crate) fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Inspect the contract runtime.
    pub(crate) fn contract_runtime(&self) -> &ContractRuntime {
        &self.contract_runtime
    }
}

#[cfg(test)]
mod tests {
    use casper_types::{EraId, ProtocolVersion};

    use crate::{
        components::linear_chain_sync::LinearChainSyncError,
        contract_runtime::BlockExecutionError,
        effect::EffectBuilder,
        reactor::{EventQueueHandle, QueueKind, Scheduler},
        testing::TestRng,
        types::Block,
        utils::{self, SharedFlag},
    };

    use super::*;

    #[tokio::test]
    async fn generates_joiner_event() {
        let scheduler = utils::leak(Scheduler::new(QueueKind::weights()));
        let event_queue = EventQueueHandle::new(scheduler, SharedFlag::default());
        let effect_builder = EffectBuilder::new(event_queue);

        let mut rng = TestRng::new();
        let era_id = EraId::new(42);
        let height = 1;
        let protocol_version = ProtocolVersion::from_parts(1, 2, 3);
        let is_switch = false;

        let block =
            Block::random_with_specifics(&mut rng, era_id, height, protocol_version, is_switch);

        // Ok(_) should result in generating JoinerEvent::FinishedJoining
        let result_ok = Ok(block.header().clone());
        let joiner_event = generate_joiner_event(result_ok, effect_builder).await;
        assert!(matches!(
            joiner_event,
            Some(JoinerEvent::FinishedJoining { block_header: _ })
        ));

        // Block from the future version should result in generating JoinerEvent::Shutdown
        // with proper exit code.
        let result_block_from_future = Err(
            LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                current_version: protocol_version,
                block_header_with_future_version: Box::new(block.header().clone()),
            },
        );
        let joiner_event = generate_joiner_event(result_block_from_future, effect_builder).await;
        assert!(matches!(
            joiner_event,
            Some(JoinerEvent::Shutdown(ExitCode::Success))
        ));

        // CurrentBlockHeaderHasOldVersion response should no longer generate JoinerEvent::Shutdown,
        // but None. See: https://github.com/casper-network/casper-node/issues/2338
        let result_block_from_past = Err(LinearChainSyncError::CurrentBlockHeaderHasOldVersion {
            current_version: protocol_version,
            block_header_with_old_version: Box::new(block.header().clone()),
        });
        let joiner_event = generate_joiner_event(result_block_from_past, effect_builder).await;
        assert!(matches!(joiner_event, None));

        // Other errors should result in None, we test against arbitrarily selected one.
        let arbitrary_error = Err(LinearChainSyncError::BlockExecutionError(
            BlockExecutionError::MoreThanOneExecutionResult,
        ));
        let joiner_event = generate_joiner_event(arbitrary_error, effect_builder).await;
        assert!(matches!(joiner_event, None));
    }
}
