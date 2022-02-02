//! Reactor used to join the network.

mod memory_metrics;

use std::{
    borrow::Cow,
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use datasize::DataSize;
use derive_more::From;
use memory_metrics::MemoryMetrics;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tracing::{debug, warn};

use casper_execution_engine::storage::trie::TrieOrChunk;

#[cfg(test)]
use crate::testing::network::NetworkedReactor;
use crate::{
    components::{
        chain_synchronizer::{self, ChainSynchronizer, JoiningOutcome},
        chainspec_loader::{self, ChainspecLoader},
        console::{self, Console},
        contract_runtime::ContractRuntime,
        deploy_acceptor::{self, DeployAcceptor},
        event_stream_server,
        event_stream_server::{DeployGetter, EventStreamServer},
        fetcher::{self, Fetcher, FetcherBuilder, TrieFetcher, TrieFetcherEvent},
        gossiper::{self, Gossiper},
        metrics::Metrics,
        rest_server::{self, RestServer},
        small_network::{self, GossipedAddress, SmallNetwork, SmallNetworkIdentity},
        storage::{self, Storage},
        Component,
    },
    contract_runtime,
    effect::{
        announcements::{
            BlocklistAnnouncement, ChainspecLoaderAnnouncement, ContractRuntimeAnnouncement,
            ControlAnnouncement, DeployAcceptorAnnouncement, GossiperAnnouncement,
            LinearChainAnnouncement,
        },
        console::DumpConsensusStateRequest,
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
        EffectBuilder, EffectExt, Effects,
    },
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
    },
    utils::WithDir,
    NodeRng,
};

/// Top-level event for the reactor.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, From, Serialize)]
#[must_use]
pub(crate) enum JoinerEvent {
    /// Chain synchronizer event.
    #[from]
    ChainSynchronizer(chain_synchronizer::Event),

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

    /// Address gossiper event.
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),

    // Requests.
    /// Storage request.
    #[from]
    StorageRequest(StorageRequest),

    /// Console event.
    #[from]
    Console(console::Event),

    /// Contract runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),

    /// Requests.
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

    /// Contract runtime request.
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),

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
    /// Consensus dump request.
    #[from]
    DumpConsensusStateRequest(DumpConsensusStateRequest),
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
            JoinerEvent::ChainSynchronizer(_) => "ChainSynchronizer",
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
            JoinerEvent::Console(_) => "Console",
            JoinerEvent::BlockFetcherRequest(_) => "BlockFetcherRequest",
            JoinerEvent::BlockByHeightFetcherRequest(_) => "BlockByHeightFetcherRequest",
            JoinerEvent::DeployFetcherRequest(_) => "DeployFetcherRequest",
            JoinerEvent::TrieOrChunkFetcherRequest(_) => "TrieOrChunkFetcherRequest",
            JoinerEvent::TrieFetcherRequest(_) => "TrieFetcherRequest",
            JoinerEvent::DumpConsensusStateRequest(_) => "DumpConsensusStateRequest",
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
            JoinerEvent::ContractRuntimeRequest(_) => "ContractRuntimeRequest",
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
            JoinerEvent::ChainSynchronizer(event) => {
                write!(f, "chain synchronizer: {}", event)
            }
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
            JoinerEvent::Console(event) => write!(f, "console: {}", event),
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
            JoinerEvent::ConsensusMessageIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::DeployGossiperIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::AddressGossiperIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::NetRequestIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::NetResponseIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::TrieRequestIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::TrieResponseIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::FinalitySignatureIncoming(inner) => write!(f, "incoming: {}", inner),
            JoinerEvent::ContractRuntimeRequest(req) => {
                write!(f, "contract runtime request: {}", req)
            }
            JoinerEvent::DumpConsensusStateRequest(req) => {
                write!(f, "consensus dump request: {}", req)
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
    chain_synchronizer: ChainSynchronizer,
    deploy_fetcher: Fetcher<Deploy>,
    block_by_hash_fetcher: Fetcher<Block>,
    block_by_height_fetcher: Fetcher<BlockWithMetadata>,
    block_header_and_finality_signatures_by_height_fetcher: Fetcher<BlockHeaderWithMetadata>,
    trie_or_chunk_fetcher: Fetcher<TrieOrChunk>,
    // Handles requests for fetching tries from the network.
    trie_fetcher: TrieFetcher<NodeId>,
    console: Console,
    block_header_by_hash_fetcher: Fetcher<BlockHeader>,
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

        let chainspec = chainspec_loader.chainspec().as_ref();
        let (console, console_effects) =
            Console::new(&WithDir::new(&root, config.console.clone()), event_queue)?;

        let (small_network, small_network_effects) = SmallNetwork::new(
            event_queue,
            config.network.clone(),
            Some(WithDir::new(&root, &config.consensus)),
            registry,
            small_network_identity,
            chainspec,
        )?;

        let mut effects = reactor::wrap_effects(JoinerEvent::SmallNetwork, small_network_effects);
        effects.extend(reactor::wrap_effects(JoinerEvent::Console, console_effects));

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let effect_builder = EffectBuilder::new(event_queue);
        let (chain_synchronizer, sync_effects) = ChainSynchronizer::new(
            Arc::clone(chainspec_loader.chainspec()),
            config.node.clone(),
            effect_builder,
            registry
        );
        effects.extend(reactor::wrap_effects(
            JoinerEvent::ChainSynchronizer,
            sync_effects,
        ));

        let verifiable_chunked_hash_activation = chainspec_loader
            .chainspec()
            .protocol_config
            .verifiable_chunked_hash_activation;
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

        let fetcher_builder =
            FetcherBuilder::new(config.fetcher, registry, verifiable_chunked_hash_activation);

        let deploy_fetcher = fetcher_builder.build("deploy")?;
        let block_by_height_fetcher = fetcher_builder.build("block_by_height")?;
        let block_by_hash_fetcher = fetcher_builder.build("block")?;
        let block_header_and_finality_signatures_by_height_fetcher =
            fetcher_builder.build("block_header_by_height")?;
        let block_header_by_hash_fetcher = fetcher_builder.build("block_header")?;

        let trie_or_chunk_fetcher = fetcher_builder.build("trie_or_chunk")?;
        let trie_fetcher = TrieFetcher::new(verifiable_chunked_hash_activation);

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
                chain_synchronizer,
                block_by_hash_fetcher,
                deploy_fetcher,
                block_by_height_fetcher,
                block_header_by_hash_fetcher,
                block_header_and_finality_signatures_by_height_fetcher,
                trie_or_chunk_fetcher,
                trie_fetcher,
                deploy_acceptor,
                event_queue_metrics,
                rest_server,
                event_stream_server,
                memory_metrics,
                node_startup_instant,
                console,
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
            JoinerEvent::ChainSynchronizer(event) => reactor::wrap_effects(
                JoinerEvent::ChainSynchronizer,
                self.chain_synchronizer
                    .handle_event(effect_builder, rng, event),
            ),
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
                    verifiable_chunked_hash_activation: None,
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
            JoinerEvent::ContractRuntimeRequest(req) => reactor::wrap_effects(
                JoinerEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, req.into()),
            ),
            JoinerEvent::ContractRuntimeAnnouncement(_) => Effects::new(),
            JoinerEvent::AddressGossiper(event) => reactor::wrap_effects(
                JoinerEvent::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::Console(event) => reactor::wrap_effects(
                JoinerEvent::Console,
                self.console.handle_event(effect_builder, rng, event),
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
            JoinerEvent::NetRequestIncoming(incoming) => reactor::wrap_effects(
                JoinerEvent::Storage,
                self.storage
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            JoinerEvent::NetResponseIncoming(NetResponseIncoming { sender, message }) => {
                self.handle_get_response(effect_builder, rng, sender, message)
            }
            JoinerEvent::TrieRequestIncoming(incoming) => reactor::wrap_effects(
                JoinerEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
            JoinerEvent::TrieResponseIncoming(TrieResponseIncoming { sender, message }) => {
                reactor::handle_fetch_response::<Self, TrieOrChunk>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    &message.0,
                    self.chainspec_loader
                        .chainspec()
                        .protocol_config
                        .verifiable_chunked_hash_activation,
                )
            }

            JoinerEvent::FinalitySignatureIncoming(FinalitySignatureIncoming {
                sender, ..
            }) => {
                debug!(%sender, "finality signatures not handled in joiner reactor");
                Effects::new()
            }
            JoinerEvent::DumpConsensusStateRequest(req) => {
                // We have no consensus running in the joiner, so we answer with `None`.
                req.answer(Err(Cow::Borrowed("node is joining, no running consensus")))
                    .ignore()
            }
        }
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        self.chain_synchronizer
            .joining_outcome()
            .map(|outcome| match outcome {
                JoiningOutcome::ShouldExitForUpgrade => {
                    ReactorExit::ProcessShouldExit(ExitCode::Success)
                }
                JoiningOutcome::Synced { .. } | JoiningOutcome::RanUpgradeOrGenesis { .. } => {
                    ReactorExit::ProcessShouldContinue
                }
            })
    }

    fn update_metrics(&mut self, event_queue_handle: EventQueueHandle<Self::Event>) {
        self.memory_metrics.estimate(self);
        self.event_queue_metrics
            .record_event_queue_counts(&event_queue_handle);
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
        let verifiable_chunked_hash_activation = self
            .chainspec_loader
            .chainspec()
            .protocol_config
            .verifiable_chunked_hash_activation;
        match message {
            NetResponse::Deploy(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, Deploy>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                    verifiable_chunked_hash_activation,
                )
            }
            NetResponse::Block(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, Block>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                    verifiable_chunked_hash_activation,
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
                    verifiable_chunked_hash_activation,
                )
            }
            NetResponse::BlockHeaderByHash(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, BlockHeader>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                    verifiable_chunked_hash_activation,
                )
            }
            NetResponse::BlockHeaderAndFinalitySignaturesByHeight(ref serialized_item) => {
                reactor::handle_fetch_response::<Self, BlockHeaderWithMetadata>(
                    self,
                    effect_builder,
                    rng,
                    sender,
                    serialized_item,
                    verifiable_chunked_hash_activation,
                )
            }
        }
    }

    /// Deconstructs the reactor into config useful for creating a Validator reactor. Shuts down
    /// the network, closing all incoming and outgoing connections, and frees up the listening
    /// socket.
    pub(crate) async fn into_participating_config(self) -> Result<ParticipatingInitConfig, Error> {
        let joining_outcome = self
            .chain_synchronizer
            .into_joining_outcome()
            .ok_or(Error::InvalidJoiningOutcome)?;
        let config = ParticipatingInitConfig {
            root: self.root,
            chainspec_loader: self.chainspec_loader,
            config: self.config,
            contract_runtime: self.contract_runtime,
            storage: self.storage,
            joining_outcome,
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
