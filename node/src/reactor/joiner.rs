//! Reactor used to join the network.

mod memory_metrics;

use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use datasize::DataSize;
use derive_more::From;
use memory_metrics::MemoryMetrics;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tracing::{debug, error, info, trace, warn};

#[cfg(test)]
use crate::testing::network::NetworkedReactor;
use crate::{
    components::{
        chainspec_loader::{self, ChainspecLoader},
        consensus::EraReport,
        contract_runtime::ContractRuntime,
        deploy_acceptor::{self, DeployAcceptor},
        event_stream_server,
        event_stream_server::{DeployGetter, EventStreamServer},
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        linear_chain_sync::{self, LinearChainSyncError, LinearChainSyncState},
        metrics::Metrics,
        rest_server::{self, RestServer},
        small_network::{self, GossipedAddress, SmallNetwork, SmallNetworkIdentity},
        storage::Storage,
        Component,
    },
    contract_runtime::ExecutionPreState,
    effect::{
        announcements::{
            BlocklistAnnouncement, ChainspecLoaderAnnouncement, ContractRuntimeAnnouncement,
            ControlAnnouncement, DeployAcceptorAnnouncement, GossiperAnnouncement,
            LinearChainAnnouncement, NetworkAnnouncement,
        },
        requests::{
            ChainspecLoaderRequest, ConsensusRequest, ContractRuntimeRequest, FetcherRequest,
            MetricsRequest, NetworkInfoRequest, NetworkRequest, RestRequest, StorageRequest,
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
    storage,
    types::{
        ActivationPoint, Block, BlockHeader, BlockHeaderWithMetadata, BlockPayload,
        BlockWithMetadata, Chainspec, Deploy, ExitCode, FinalizedBlock, NodeId, Tag, Timestamp,
    },
    utils::{Source, WithDir},
    NodeRng,
};
use casper_execution_engine::{
    core::engine_state::{GenesisSuccess, UpgradeConfig, UpgradeSuccess},
    storage::trie::Trie,
};
use casper_types::{EraId, Key, ProtocolVersion, PublicKey, StoredValue};

/// Top-level event for the reactor.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, From, Serialize)]
#[must_use]
pub(crate) enum JoinerEvent {
    /// Finished joining event.
    FinishedJoining {
        switch_block_header: Box<BlockHeader>,
    },

    /// Finished genesis event.
    FinishedGenesis {
        switch_block_header: Box<BlockHeader>,
    },

    /// Shut down with the given exit code.
    Shutdown(#[serde(skip_serializing)] ExitCode),

    /// Small Network event.
    #[from]
    SmallNetwork(small_network::Event<Message>),

    /// Storage event.
    #[from]
    Storage(#[serde(skip_serializing)] storage::Event),

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

    /// Trie fetcher event.
    #[from]
    TrieFetcher(#[serde(skip_serializing)] fetcher::Event<Trie<Key, StoredValue>>),

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

    /// Deploy acceptor event.
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] deploy_acceptor::Event),

    /// Contract Runtime event.
    #[from]
    ContractRuntime(#[serde(skip_serializing)] ContractRuntimeRequest),

    /// Address gossiper event.
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),

    /// Requests.
    /// Linear chain block by hash fetcher request.
    #[from]
    BlockFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, Block>),

    /// Trie fetcher request.
    #[from]
    TrieFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, Trie<Key, StoredValue>>),

    /// Blocker header (with no metadata) fetcher request.
    #[from]
    BlockHeaderFetcherRequest(#[serde(skip_serializing)] FetcherRequest<NodeId, BlockHeader>),

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

    // Announcements
    /// A control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),

    /// Network announcement.
    #[from]
    NetworkAnnouncement(#[serde(skip_serializing)] NetworkAnnouncement<NodeId, Message>),

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
}

impl From<StorageRequest> for JoinerEvent {
    fn from(request: StorageRequest) -> Self {
        JoinerEvent::Storage(request.into())
    }
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
            JoinerEvent::DeployAcceptor(_) => "DeployAcceptor",
            JoinerEvent::ContractRuntime(_) => "ContractRuntime",
            JoinerEvent::AddressGossiper(_) => "AddressGossiper",
            JoinerEvent::BlockFetcherRequest(_) => "BlockFetcherRequest",
            JoinerEvent::BlockByHeightFetcherRequest(_) => "BlockByHeightFetcherRequest",
            JoinerEvent::DeployFetcherRequest(_) => "DeployFetcherRequest",
            JoinerEvent::ControlAnnouncement(_) => "ControlAnnouncement",
            JoinerEvent::NetworkAnnouncement(_) => "NetworkAnnouncement",
            JoinerEvent::ContractRuntimeAnnouncement(_) => "ContractRuntimeAnnouncement",
            JoinerEvent::AddressGossiperAnnouncement(_) => "AddressGossiperAnnouncement",
            JoinerEvent::DeployAcceptorAnnouncement(_) => "DeployAcceptorAnnouncement",
            JoinerEvent::LinearChainAnnouncement(_) => "LinearChainAnnouncement",
            JoinerEvent::ChainspecLoaderAnnouncement(_) => "ChainspecLoaderAnnouncement",
            JoinerEvent::ConsensusRequest(_) => "ConsensusRequest",
            JoinerEvent::TrieFetcher(_) => "TrieFetcher",
            JoinerEvent::BlockHeaderFetcher(_) => "BlockHeaderFetcher",
            JoinerEvent::BlockHeaderByHeightFetcher(_) => "BlockHeaderByHeightFetcher",
            JoinerEvent::TrieFetcherRequest(_) => "TrieFetcherRequest",
            JoinerEvent::BlockHeaderFetcherRequest(_) => "BlockHeaderFetcherRequest",
            JoinerEvent::BlockHeaderByHeightFetcherRequest(_) => {
                "BlockHeaderByHeightFetcherRequest"
            }
            JoinerEvent::FinishedJoining { .. } => "FinishedJoining",
            JoinerEvent::Shutdown(_) => "Shutdown",
            JoinerEvent::BlocklistAnnouncement(_) => "BlocklistAnnouncement",
            JoinerEvent::FinishedGenesis {
                switch_block_header: _,
            } => "FinishedGenesis",
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
            JoinerEvent::NetworkAnnouncement(event) => write!(f, "network announcement: {}", event),
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
            JoinerEvent::NetworkInfoRequest(req) => write!(f, "network info request: {}", req),
            JoinerEvent::BlockFetcherRequest(request) => {
                write!(f, "block fetcher request: {}", request)
            }
            JoinerEvent::DeployFetcherRequest(request) => {
                write!(f, "deploy fetcher request: {}", request)
            }
            JoinerEvent::BlockFetcher(event) => write!(f, "block fetcher: {}", event),
            JoinerEvent::BlockByHeightFetcherRequest(request) => {
                write!(f, "block by height fetcher request: {}", request)
            }
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
            JoinerEvent::TrieFetcher(trie) => {
                write!(f, "trie fetcher event: {}", trie)
            }
            JoinerEvent::TrieFetcherRequest(req) => {
                write!(f, "trie fetcher request: {}", req)
            }
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
            JoinerEvent::FinishedJoining {
                switch_block_header: block_header,
            } => {
                write!(f, "finished joining with block header: {}", block_header)
            }
            JoinerEvent::Shutdown(exit_code) => {
                write!(f, "shutting down with exit code: {:?}", exit_code)
            }
            JoinerEvent::FinishedGenesis {
                switch_block_header,
            } => write!(
                f,
                "finished genesis with switch block: {}",
                switch_block_header
            ),
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
    storage: Arc<Mutex<Storage>>,
    contract_runtime: ContractRuntime,
    linear_chain_sync: LinearChainSyncState,
    deploy_fetcher: Fetcher<Deploy>,
    block_by_hash_fetcher: Fetcher<Block>,
    block_by_height_fetcher: Fetcher<BlockWithMetadata>,
    block_header_by_hash_fetcher: Fetcher<BlockHeader>,
    block_header_and_finality_signatures_by_height_fetcher: Fetcher<BlockHeaderWithMetadata>,
    /// This is set to `Some` if the node should shut down.
    exit_code: Option<ExitCode>,
    // Handles request for fetching tries from the network.
    #[data_size(skip)]
    trie_fetcher: Fetcher<Trie<Key, StoredValue>>,
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

        let storage_root_path = storage.root_path().to_path_buf();
        let storage = Arc::new(Mutex::new(storage));

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
                .lock()
                .expect("mutex poisoned")
                .read_highest_block_header()
                .expect("Could not read highest block header")
                .map(|block_header| block_header.hash()),
        };

        let chainspec = chainspec_loader.chainspec().clone();
        let linear_chain_sync = match maybe_trusted_hash {
            None => {
                if let Some(start_time) = chainspec
                    .protocol_config
                    .activation_point
                    .genesis_timestamp()
                {
                    let era_duration = chainspec.core_config.era_duration;
                    if Timestamp::now() > start_time + era_duration {
                        error!(
                            now=?Timestamp::now(),
                            genesis_era_end=?start_time + era_duration,
                            "node started with no trusted hash after the expected end of \
                             the genesis era! Please specify a trusted hash and restart.");
                        panic!("should have trusted hash after genesis era")
                    }
                };
                let chainspec = Arc::clone(chainspec_loader.chainspec());
                let contract_runtime = contract_runtime.clone();
                let storage_wrapper = Arc::clone(&storage);
                effects.extend(
                    (async move {
                        match Self::commit_upgrade_or_genesis_switch_block(
                            None,
                            chainspec,
                            storage_wrapper,
                            &contract_runtime,
                            effect_builder,
                        )
                        .await
                        {
                            Ok(switch_block_header) => Some(JoinerEvent::FinishedGenesis {
                                switch_block_header: Box::new(switch_block_header),
                            }),
                            Err(error) => {
                                fatal!(effect_builder, "{:?}", error).await;
                                None
                            }
                        }
                    })
                    .map_some(std::convert::identity),
                );
                LinearChainSyncState::Syncing
            }
            Some(hash) => {
                let node_config = config.node.clone();
                let contract_runtime = contract_runtime.clone();
                let storage_wrapper = Arc::clone(&storage);
                effects.extend(
                    (async move {
                        info!(trusted_hash=%hash, "synchronizing linear chain");
                        match linear_chain_sync::run_fast_sync_task(
                            effect_builder,
                            hash,
                            Arc::clone(&chainspec),
                            node_config,
                        )
                        .await
                        {
                            Ok(latest_block_header) => {
                                match Self::commit_upgrade_or_genesis_switch_block(
                                    Some(latest_block_header),
                                    chainspec,
                                    storage_wrapper,
                                    &contract_runtime,
                                    effect_builder,
                                )
                                .await
                                {
                                    Ok(switch_block_header) => Some(JoinerEvent::FinishedJoining {
                                        switch_block_header: Box::new(switch_block_header),
                                    }),
                                    Err(error) => {
                                        fatal!(effect_builder, "{:?}", error).await;
                                        None
                                    }
                                }
                            }

                            Err(LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                                current_version,
                                block_header_with_future_version,
                            }) => {
                                let future_version =
                                    block_header_with_future_version.protocol_version();
                                info!(%current_version, %future_version, "restarting for upgrade");
                                Some(JoinerEvent::Shutdown(ExitCode::Success))
                            }
                            Err(LinearChainSyncError::CurrentBlockHeaderHasOldVersion {
                                current_version,
                                block_header_with_old_version,
                            }) => {
                                let old_version = block_header_with_old_version.protocol_version();
                                info!(%current_version, %old_version, "restarting for downgrade");
                                Some(JoinerEvent::Shutdown(ExitCode::DowngradeVersion))
                            }
                            Err(error) => {
                                fatal!(effect_builder, "{:?}", error).await;
                                None
                            }
                        }
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
            storage_root_path,
            *protocol_version,
            DeployGetter::new(effect_builder),
        )?;

        let deploy_fetcher = Fetcher::new("deploy", config.fetcher, registry)?;

        let block_by_height_fetcher = Fetcher::new("block_by_height", config.fetcher, registry)?;

        let block_by_hash_fetcher = Fetcher::new("block", config.fetcher, registry)?;
        let trie_fetcher = Fetcher::new("trie", config.fetcher, registry)?;
        let block_header_and_finality_signatures_by_height_fetcher =
            Fetcher::new("block_header_by_height", config.fetcher, registry)?;

        let block_header_by_hash_fetcher: Fetcher<BlockHeader> =
            Fetcher::new("block_header", config.fetcher, registry)?;

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
                trie_fetcher,
                deploy_fetcher,
                block_by_height_fetcher,
                block_header_by_hash_fetcher,
                block_header_and_finality_signatures_by_height_fetcher,
                exit_code: None,
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
            JoinerEvent::NetworkAnnouncement(NetworkAnnouncement::NewPeer(_id)) => {
                Effects::new()
            }
            JoinerEvent::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(
                gossiped_address,
            )) => {
                let event = gossiper::Event::ItemReceived {
                    item_id: gossiped_address,
                    source: Source::<NodeId>::Ourself,
                };
                self.dispatch_event(effect_builder, rng, JoinerEvent::AddressGossiper(event))
            }
            JoinerEvent::BlocklistAnnouncement(ann) => {
                self.dispatch_event(effect_builder, rng, JoinerEvent::SmallNetwork(ann.into()))
            }
            JoinerEvent::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => match payload {
                Message::GetResponse {
                    tag: Tag::Block,
                    serialized_item
                } => {
                    match fetcher::Event::<Block>::from_get_response_serialized_item(
                        sender,
                        &serialized_item,
                    ) {
                        Some(fetcher_event) => {
                            self.dispatch_event(effect_builder, rng, JoinerEvent::BlockFetcher(fetcher_event))
                        } ,
                        None => {
                            info!("{} sent us a block we couldn't parse! Banning", sender);
                            effect_builder.announce_disconnect_from_peer(sender).ignore()
                        }
                    }
                }
                Message::GetResponse {
                    tag: Tag::BlockAndMetadataByHeight,
                    serialized_item,
                } => {
                    match fetcher::Event::<BlockWithMetadata>::from_get_response_serialized_item(
                        sender,
                        &serialized_item,
                    ) {
                        Some(fetcher_event) => {
                            self.dispatch_event(effect_builder, rng, JoinerEvent::BlockByHeightFetcher(fetcher_event))
                        }
                        None => {
                            info!("{} sent us a block with metadata we couldn't parse! Banning", sender);
                            effect_builder.announce_disconnect_from_peer(sender).ignore()
                        }
                    }
                }
                Message::GetResponse {
                    tag: Tag::Trie,
                    serialized_item,
                } => {
                    match fetcher::Event::<Trie<Key, StoredValue>>::from_get_response_serialized_item(
                        sender,
                        &serialized_item,
                    ) {
                        Some(fetcher_event) => {
                            self.dispatch_event(effect_builder, rng, JoinerEvent::TrieFetcher(fetcher_event))
                        } ,
                        None => {
                            info!("{} sent us a trie we couldn't parse! Banning", sender);
                            effect_builder.announce_disconnect_from_peer(sender).ignore()
                        }
                    }
                }
                Message::GetResponse {
                    tag: Tag::BlockHeaderByHash,
                    serialized_item,
                } => {
                    match fetcher::Event::<BlockHeader>::from_get_response_serialized_item(
                        sender,
                        &serialized_item,
                    ) {
                        Some(fetcher_event) => {
                            self.dispatch_event(effect_builder, rng, JoinerEvent::BlockHeaderFetcher(fetcher_event))
                        } ,
                        None => {
                            info!("{} sent us a block header we couldn't parse! Banning", sender);
                            effect_builder.announce_disconnect_from_peer(sender).ignore()
                        }
                    }
                }
                Message::GetResponse {
                    tag: Tag::BlockHeaderAndFinalitySignaturesByHeight,
                    serialized_item,
                } => {
                    match fetcher::Event::<BlockHeaderWithMetadata>::from_get_response_serialized_item(
                        sender,
                        &serialized_item,
                    ) {
                        Some(fetcher_event) => {
                            self.dispatch_event(effect_builder, rng, JoinerEvent::BlockHeaderByHeightFetcher(fetcher_event))
                        } ,
                        None => {
                            info!("{} sent us a block header with finality signatures we couldn't parse! Banning", sender);
                            effect_builder.announce_disconnect_from_peer(sender).ignore()
                        }
                    }
                }
                Message::GetResponse {
                    tag: Tag::Deploy,
                    serialized_item,
                } => {
                    match fetcher::Event::<Deploy>::from_get_response_serialized_item(
                        sender,
                        &serialized_item,
                    ) {
                        Some(fetcher_event) => {
                            self.dispatch_event(effect_builder, rng, JoinerEvent::DeployFetcher(fetcher_event))
                        },
                        None => {
                            info!("{} sent us a deploy we couldn't parse! Banning", sender);
                            effect_builder.announce_disconnect_from_peer(sender).ignore()
                        }
                    }
                }
                Message::AddressGossiper(message) => {
                    let event = JoinerEvent::AddressGossiper(gossiper::Event::MessageReceived {
                        sender,
                        message,
                    });
                    self.dispatch_event(effect_builder, rng, event)
                }
                Message::FinalitySignature(_) => {
                    debug!("finality signatures not handled in joiner reactor");
                    Effects::new()
                }
                other => {
                    debug!(?other, "network announcement ignored.");
                    Effects::new()
                }
            },
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
                self.storage.lock().expect("mutex poisoned").handle_event(effect_builder, rng, event)
            ),
            JoinerEvent::BlockFetcherRequest(request) => {
                self.dispatch_event(effect_builder, rng, JoinerEvent::BlockFetcher(request.into()))
            }
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
            JoinerEvent::TrieFetcher(event) => reactor::wrap_effects(
                JoinerEvent::TrieFetcher,
                self.trie_fetcher.handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::BlockHeaderFetcher(event) => reactor::wrap_effects(
                JoinerEvent::BlockHeaderFetcher,
                self.block_header_by_hash_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::DeployFetcherRequest(request) => {
                self.dispatch_event(effect_builder, rng, JoinerEvent::DeployFetcher(request.into()))
            }
            JoinerEvent::BlockByHeightFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::BlockByHeightFetcher(request.into()),
            ),
            JoinerEvent::TrieFetcherRequest(request) => {
                self.dispatch_event(effect_builder, rng, JoinerEvent::TrieFetcher(request.into()))
            }
            JoinerEvent::BlockHeaderFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                JoinerEvent::BlockHeaderFetcher(request.into()),
            ),
            JoinerEvent::ContractRuntime(event) => reactor::wrap_effects(
                JoinerEvent::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            JoinerEvent::ContractRuntimeAnnouncement(_) => {
                Effects::new()
            }
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
            JoinerEvent::FinishedJoining { switch_block_header } => {
                self.linear_chain_sync = LinearChainSyncState::Done{ switch_block_header };
                Effects::new()
            }
            JoinerEvent::FinishedGenesis { switch_block_header } => {
                self.linear_chain_sync = LinearChainSyncState::NotGoingToSync{ switch_block_header };
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

impl Reactor {
    /// Deconstructs the reactor into config useful for creating a Validator reactor. Shuts down
    /// the network, closing all incoming and outgoing connections, and frees up the listening
    /// socket.
    pub(crate) async fn into_participating_config(self) -> Result<ParticipatingInitConfig, Error> {
        let latest_switch_block_header = match self.linear_chain_sync {
            LinearChainSyncState::Syncing => return Err(Error::StillSyncing),
            LinearChainSyncState::Done {
                switch_block_header,
            }
            | LinearChainSyncState::NotGoingToSync {
                switch_block_header,
            } => *switch_block_header,
        };
        let storage = Arc::try_unwrap(self.storage).map_err(|_| Error::StorageOwnership)?;
        let storage = Mutex::into_inner(storage).map_err(|_| Error::StorageOwnership)?;
        let config = ParticipatingInitConfig {
            root: self.root,
            chainspec_loader: self.chainspec_loader,
            config: self.config,
            contract_runtime: self.contract_runtime,
            storage,
            latest_switch_block_header,
            event_stream_server: self.event_stream_server,
            small_network_identity: SmallNetworkIdentity::from(&self.small_network),
            node_startup_instant: self.node_startup_instant,
        };
        self.small_network.finalize().await;
        self.rest_server.finalize().await;
        Ok(config)
    }

    // If we have a latest block header, we joined via the joiner reactor.
    //
    // If not, run genesis or upgrade and construct a switch block, and use that for the latest
    // block header.
    async fn commit_upgrade_or_genesis_switch_block(
        maybe_latest_block_header: Option<BlockHeader>,
        chainspec: Arc<Chainspec>,
        storage: Arc<Mutex<Storage>>,
        contract_runtime: &ContractRuntime,
        effect_builder: EffectBuilder<JoinerEvent>,
    ) -> Result<BlockHeader, Error> {
        let block_header = match maybe_latest_block_header {
            Some(latest_block_header)
                if latest_block_header.protocol_version() == chainspec.protocol_config.version =>
            {
                latest_block_header
            }
            Some(latest_block_header)
                if chainspec
                    .protocol_config
                    .is_last_block_before_activation(&latest_block_header) =>
            {
                Self::commit_upgrade_switch_block(
                    latest_block_header,
                    chainspec,
                    storage,
                    contract_runtime,
                    effect_builder,
                )
                .await?
            }
            Some(latest_block_header) => {
                return Err(Error::UnexpectedLatestBlockHeader {
                    latest_block_header: Box::new(latest_block_header),
                });
            }
            None => match chainspec.protocol_config.activation_point {
                ActivationPoint::EraId(upgrade_era_id) => {
                    return Err(Error::NoSuchSwitchBlockHeaderForUpgradeEra { upgrade_era_id });
                }
                ActivationPoint::Genesis(genesis_timestamp) => {
                    // Do not run genesis on a node which is not protocol version 1.0.0
                    Self::commit_genesis_switch_block(
                        chainspec,
                        storage,
                        contract_runtime,
                        effect_builder,
                        genesis_timestamp,
                    )
                    .await?
                }
            },
        };
        Ok(block_header)
    }

    async fn commit_genesis_switch_block(
        chainspec: Arc<Chainspec>,
        storage: Arc<Mutex<Storage>>,
        contract_runtime: &ContractRuntime,
        effect_builder: EffectBuilder<JoinerEvent>,
        genesis_timestamp: Timestamp,
    ) -> Result<BlockHeader, Error> {
        if chainspec.protocol_config.version != ProtocolVersion::V1_0_0 {
            return Err(Error::GenesisNeedsProtocolVersion1_0_0 {
                chainspec_protocol_version: chainspec.protocol_config.version,
            });
        }
        if let Some(highest_block_header) = storage
            .lock()
            .expect("mutex poisoned")
            .read_highest_block_header()?
        {
            return Err(Error::CannotRunGenesisOnPreExistingBlockchain {
                highest_block_header: Box::new(highest_block_header),
            });
        }
        let GenesisSuccess {
            post_state_hash,
            execution_effect,
        } = contract_runtime.commit_genesis(&chainspec)?;
        info!("genesis chainspec name {}", chainspec.network_config.name);
        info!("genesis state root hash {}", post_state_hash);
        trace!(%post_state_hash, ?execution_effect);
        let initial_pre_state =
            ExecutionPreState::new(0, post_state_hash, Default::default(), Default::default());
        let new_header = Self::create_immediate_switch_block(
            contract_runtime,
            storage,
            effect_builder,
            &chainspec,
            initial_pre_state,
            genesis_timestamp,
            EraId::from(0u64),
        )
        .await?;
        Ok(new_header)
    }

    async fn commit_upgrade_switch_block(
        latest_block_header: BlockHeader,
        chainspec: Arc<Chainspec>,
        storage_wrapper: Arc<Mutex<Storage>>,
        contract_runtime: &ContractRuntime,
        effect_builder: EffectBuilder<JoinerEvent>,
    ) -> Result<BlockHeader, Error> {
        let upgrade_block_header = latest_block_header;
        let upgrade_era_id = upgrade_block_header.next_block_era_id();
        if chainspec.protocol_config.last_emergency_restart != Some(upgrade_block_header.era_id()) {
            if let Some(preexisting_block_header) = storage_wrapper
                .lock()
                .expect("mutex poisoned")
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
        let initial_pre_state = ExecutionPreState::new(
            upgrade_block_header.height() + 1,
            post_state_hash,
            upgrade_block_header.hash(),
            upgrade_block_header.accumulated_seed(),
        );
        let new_header = Self::create_immediate_switch_block(
            contract_runtime,
            storage_wrapper,
            effect_builder,
            &chainspec,
            initial_pre_state,
            upgrade_block_header.timestamp(),
            upgrade_era_id,
        )
        .await?;
        Ok(new_header)
    }

    /// Creates a switch block after an upgrade or genesis. This block has the system public key as
    /// a proposer and doesn't contain any deploys or transfers. It is the only block in its
    /// era, and no consensus instance is run for era 0 or an upgrade point era.
    async fn create_immediate_switch_block(
        contract_runtime: &ContractRuntime,
        storage: Arc<Mutex<Storage>>,
        effect_builder: EffectBuilder<JoinerEvent>,
        chainspec: &Chainspec,
        pre_state: ExecutionPreState,
        timestamp: Timestamp,
        era_id: EraId,
    ) -> Result<BlockHeader, Error> {
        let finalized_block = FinalizedBlock::new(
            BlockPayload::default(),
            Some(EraReport::default()),
            timestamp,
            era_id,
            pre_state.next_block_height(),
            PublicKey::System,
        );
        // Execute the finalized block, creating a new switch block.
        let new_switch_block = contract_runtime
            .execute_finalized_block(
                effect_builder,
                chainspec.protocol_version(),
                finalized_block,
                vec![],
                vec![],
            )
            .await?;
        // Make sure the new block really is a switch block
        if !new_switch_block.header().is_switch_block() {
            return Err(Error::FailedToCreateSwitchBlockAfterGenesisOrUpgrade {
                new_bad_block: Box::new(new_switch_block),
            });
        }
        // Write the block to storage so the era supervisor can be initialized properly.
        storage
            .lock()
            .expect("mutex poisoned")
            .write_block(&new_switch_block)?;
        Ok(new_switch_block.take_header())
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
