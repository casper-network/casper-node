//! Reactor for participating nodes.
//!
//! Participating nodes join the participating-only network upon startup.

mod config;
mod error;
mod memory_metrics;
#[cfg(test)]
mod tests;

use std::{
    env,
    fmt::{self, Debug, Display, Formatter},
    path::PathBuf,
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tracing::{debug, error, info, trace, warn};

#[cfg(test)]
use crate::testing::network::NetworkedReactor;

use casper_execution_engine::core::engine_state::{GenesisSuccess, UpgradeConfig, UpgradeSuccess};
use casper_types::{EraId, PublicKey};

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
        network::{self, Network, NetworkIdentity, ENABLE_LIBP2P_NET_ENV_VAR},
        rest_server::{self, RestServer},
        rpc_server::{self, RpcServer},
        small_network::{self, GossipedAddress, SmallNetwork, SmallNetworkIdentity},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            BlocklistAnnouncement, ChainspecLoaderAnnouncement, ConsensusAnnouncement,
            ContractRuntimeAnnouncement, ControlAnnouncement, DeployAcceptorAnnouncement,
            GossiperAnnouncement, LinearChainAnnouncement, LinearChainBlock, NetworkAnnouncement,
            RpcServerAnnouncement,
        },
        requests::{
            BlockProposerRequest, BlockValidationRequest, ChainspecLoaderRequest, ConsensusRequest,
            ContractRuntimeRequest, FetcherRequest, MetricsRequest, NetworkInfoRequest,
            NetworkRequest, RestRequest, RpcRequest, StateStoreRequest, StorageRequest,
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

pub use config::Config;
pub use error::Error;
use linear_chain::LinearChainComponent;
use memory_metrics::MemoryMetrics;

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
pub enum Event {
    /// Network event.
    #[from]
    Network(network::Event<Message>),
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
    /// Request for state storage.
    #[from]
    StateStoreRequest(StateStoreRequest),

    // Announcements
    /// Control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    /// Network announcement.
    #[from]
    NetworkAnnouncement(#[serde(skip_serializing)] NetworkAnnouncement<NodeId, Message>),
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
}

impl ReactorEvent for Event {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

impl From<ContractRuntimeRequest> for Event {
    fn from(contract_runtime_request: ContractRuntimeRequest) -> Self {
        Event::ContractRuntime(Box::new(contract_runtime_request))
    }
}

impl From<RpcRequest<NodeId>> for Event {
    fn from(request: RpcRequest<NodeId>) -> Self {
        Event::RpcServer(rpc_server::Event::RpcRequest(request))
    }
}

impl From<RestRequest<NodeId>> for Event {
    fn from(request: RestRequest<NodeId>) -> Self {
        Event::RestServer(rest_server::Event::RestRequest(request))
    }
}

impl From<NetworkRequest<NodeId, consensus::ConsensusMessage>> for Event {
    fn from(request: NetworkRequest<NodeId, consensus::ConsensusMessage>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<Deploy>>> for Event {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<Deploy>>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>> for Event {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<ConsensusRequest> for Event {
    fn from(request: ConsensusRequest) -> Self {
        Event::Consensus(consensus::Event::ConsensusRequest(request))
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(f, "network: {}", event),
            Event::SmallNetwork(event) => write!(f, "small network: {}", event),
            Event::BlockProposer(event) => write!(f, "block proposer: {}", event),
            Event::Storage(event) => write!(f, "storage: {}", event),
            Event::RpcServer(event) => write!(f, "rpc server: {}", event),
            Event::RestServer(event) => write!(f, "rest server: {}", event),
            Event::EventStreamServer(event) => write!(f, "event stream server: {}", event),
            Event::ChainspecLoader(event) => write!(f, "chainspec loader: {}", event),
            Event::Consensus(event) => write!(f, "consensus: {}", event),
            Event::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            Event::DeployFetcher(event) => write!(f, "deploy fetcher: {}", event),
            Event::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            Event::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            Event::ContractRuntime(event) => write!(f, "contract runtime: {:?}", event),
            Event::LinearChain(event) => write!(f, "linear-chain event {}", event),
            Event::BlockValidator(event) => write!(f, "block validator: {}", event),
            Event::NetworkRequest(req) => write!(f, "network request: {}", req),
            Event::NetworkInfoRequest(req) => write!(f, "network info request: {}", req),
            Event::ChainspecLoaderRequest(req) => write!(f, "chainspec loader request: {}", req),
            Event::StorageRequest(req) => write!(f, "storage request: {}", req),
            Event::StateStoreRequest(req) => write!(f, "state store request: {}", req),
            Event::DeployFetcherRequest(req) => write!(f, "deploy fetcher request: {}", req),
            Event::BlockProposerRequest(req) => write!(f, "block proposer request: {}", req),
            Event::BlockValidatorRequest(req) => {
                write!(f, "block validator request: {}", req)
            }
            Event::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            Event::ControlAnnouncement(ctrl_ann) => write!(f, "control: {}", ctrl_ann),
            Event::NetworkAnnouncement(ann) => write!(f, "network announcement: {}", ann),
            Event::RpcServerAnnouncement(ann) => write!(f, "api server announcement: {}", ann),
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            Event::ConsensusAnnouncement(ann) => write!(f, "consensus announcement: {}", ann),
            Event::ContractRuntimeAnnouncement(ann) => {
                write!(f, "block-executor announcement: {}", ann)
            }
            Event::DeployGossiperAnnouncement(ann) => {
                write!(f, "deploy gossiper announcement: {}", ann)
            }
            Event::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
            Event::LinearChainAnnouncement(ann) => write!(f, "linear chain announcement: {}", ann),
            Event::ChainspecLoaderAnnouncement(ann) => {
                write!(f, "chainspec loader announcement: {}", ann)
            }
            Event::BlocklistAnnouncement(ann) => {
                write!(f, "blocklist announcement: {}", ann)
            }
        }
    }
}

/// The configuration needed to initialize a Participating reactor
pub struct ParticipatingInitConfig {
    pub(super) root: PathBuf,
    pub(super) config: Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
    pub(super) maybe_latest_block_header: Option<BlockHeader>,
    pub(super) event_stream_server: EventStreamServer,
    pub(super) small_network_identity: SmallNetworkIdentity,
    pub(super) network_identity: NetworkIdentity,
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
pub struct Reactor {
    metrics: Metrics,
    small_network: SmallNetwork<Event, Message>,
    network: Network<Event, Message>,
    address_gossiper: Gossiper<GossipedAddress, Event>,
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
    deploy_gossiper: Gossiper<Deploy, Event>,
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
            Tag::Deploy => {
                Self::respond_to_fetch(effect_builder, serialized_id, sender, |deploy_hash| {
                    self.storage.get_deploy(deploy_hash)
                })
            }
            Tag::Block => {
                Self::respond_to_fetch(effect_builder, serialized_id, sender, |block_hash| {
                    self.storage.read_block(&block_hash)
                })
            }
            Tag::BlockAndMetadataByHeight => {
                Self::respond_to_fetch(effect_builder, serialized_id, sender, |block_height| {
                    let chainspec = self.chainspec_loader.chainspec();
                    self.storage
                        .read_block_and_sufficient_finality_signatures_by_height(
                            block_height,
                            chainspec.highway_config.finality_threshold_fraction,
                            chainspec.protocol_config.last_emergency_restart,
                        )
                })
            }
            Tag::GossipedAddress => {
                warn!("received get request for gossiped-address from {}", sender);
                Effects::new()
            }
            Tag::BlockHeaderByHash => {
                Self::respond_to_fetch(effect_builder, serialized_id, sender, |block_hash| {
                    self.storage.get_block_header_by_hash(&block_hash)
                })
            }
            Tag::BlockHeaderAndFinalitySignaturesByHeight => {
                Self::respond_to_fetch(effect_builder, serialized_id, sender, |block_height| {
                    let chainspec = self.chainspec_loader.chainspec();
                    self.storage
                        .read_block_header_and_sufficient_finality_signatures_by_height(
                            block_height,
                            chainspec.highway_config.finality_threshold_fraction,
                            chainspec.protocol_config.last_emergency_restart,
                        )
                })
            }
            Tag::Trie => {
                Self::respond_to_fetch(effect_builder, serialized_id, sender, |trie_key| {
                    self.contract_runtime.read_trie(trie_key)
                })
            }
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
    type Event = Event;

    // The "configuration" is in fact the whole state of the joiner reactor, which we
    // deconstruct and reuse.
    type Config = ParticipatingInitConfig;
    type Error = Error;

    fn new(
        config: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Event>), Error> {
        let ParticipatingInitConfig {
            root,
            config,
            chainspec_loader,
            mut storage,
            mut contract_runtime,
            maybe_latest_block_header,
            event_stream_server,
            small_network_identity,
            network_identity,
        } = config;

        let memory_metrics = MemoryMetrics::new(registry.clone())?;

        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let metrics = Metrics::new(registry.clone());

        let effect_builder = EffectBuilder::new(event_queue);
        let network_config = network::Config::from(&config.network);
        let (network, network_effects) = Network::new(
            event_queue,
            network_config,
            registry,
            network_identity,
            chainspec_loader.chainspec(),
        )?;

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let protocol_version = &chainspec_loader.chainspec().protocol_config.version;
        let rpc_server =
            RpcServer::new(config.rpc_server.clone(), effect_builder, *protocol_version)?;
        let rest_server = RestServer::new(
            config.rest_server.clone(),
            effect_builder,
            *protocol_version,
        )?;

        let deploy_acceptor =
            DeployAcceptor::new(config.deploy_acceptor, &*chainspec_loader.chainspec());
        let deploy_fetcher = Fetcher::new("deploy", config.fetcher, registry)?;
        let deploy_gossiper = Gossiper::new_for_partial_items(
            "deploy_gossiper",
            config.gossip,
            gossiper::get_deploy_from_storage::<Deploy, Event>,
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
            match chainspec_loader
                .chainspec()
                .protocol_config
                .activation_point
            {
                ActivationPoint::Genesis(genesis_timestamp) => {
                    // Check that no blocks exist in storage so we don't try to run genesis again on
                    // an existing blockchain
                    if let Some(first_block_header) = storage.read_block_header_by_height(1)? {
                        return Err(Error::CannotRunGenesisOnPreExistingBlockchain {
                            first_block_header: Box::new(first_block_header),
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
                        post_state_hash.into(),
                        0,
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
                    let chainspec = chainspec_loader.chainspec();
                    let global_state_update = chainspec.protocol_config.get_update_mapping()?;
                    let UpgradeSuccess {
                        post_state_hash,
                        execution_effect,
                    } = contract_runtime.commit_upgrade(UpgradeConfig::new(
                        (*upgrade_block_header.state_root_hash()).into(),
                        upgrade_block_header.protocol_version(),
                        chainspec.protocol_version(),
                        Some(chainspec.wasm_config),
                        Some(chainspec.system_costs_config),
                        Some(chainspec.protocol_config.activation_point.era_id()),
                        Some(chainspec.core_config.validator_slots),
                        Some(chainspec.core_config.auction_delay),
                        Some(chainspec.core_config.locked_funds_period.millis()),
                        Some(chainspec.core_config.round_seigniorage_rate),
                        Some(chainspec.core_config.unbonding_delay),
                        global_state_update,
                    ))?;
                    info!("upgrade chainspec name {}", chainspec.network_config.name);
                    info!("upgrade state root hash {}", post_state_hash);
                    trace!(%post_state_hash, ?execution_effect);
                    initial_pre_state = ExecutionPreState::new(
                        post_state_hash.into(),
                        upgrade_block_header.height() + 1,
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
            if new_switch_block.header().era_end().is_none() {
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
        )?;
        effects.extend(reactor::wrap_effects(
            Event::BlockProposer,
            block_proposer_effects,
        ));

        let (small_network, small_network_effects) = {
            let maybe_initial_validators_hash_set = latest_block_header
                .next_era_validator_weights()
                .cloned()
                .map(|validator_weights| validator_weights.into_keys().collect());
            SmallNetwork::new(
                event_queue,
                config.network,
                Some(WithDir::new(&root, &config.consensus)),
                registry,
                small_network_identity,
                chainspec_loader.chainspec().as_ref(),
                maybe_initial_validators_hash_set,
            )?
        };

        let (consensus, init_consensus_effects) = EraSupervisor::new(
            latest_block_header.next_block_era_id(),
            WithDir::new(root, config.consensus),
            effect_builder,
            chainspec_loader.chainspec().as_ref().into(),
            &latest_block_header,
            maybe_next_activation_point,
            registry,
            Box::new(HighwayProtocol::new_boxed),
            &storage,
            rng,
        )?;
        effects.extend(reactor::wrap_effects(
            Event::Consensus,
            init_consensus_effects,
        ));

        contract_runtime.set_initial_state(ExecutionPreState::from(&latest_block_header));

        let block_validator = BlockValidator::new(Arc::clone(chainspec_loader.chainspec()));
        let linear_chain = linear_chain::LinearChainComponent::new(
            registry,
            *protocol_version,
            chainspec_loader.chainspec().core_config.auction_delay,
            chainspec_loader.chainspec().core_config.unbonding_delay,
        )?;

        effects.extend(reactor::wrap_effects(Event::Network, network_effects));
        effects.extend(reactor::wrap_effects(
            Event::SmallNetwork,
            small_network_effects,
        ));
        effects.extend(reactor::wrap_effects(
            Event::ChainspecLoader,
            chainspec_loader.start_checking_for_upgrades(effect_builder),
        ));

        event_stream_server.set_participating_effect_builder(effect_builder);

        Ok((
            Reactor {
                metrics,
                network,
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
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network.handle_event(effect_builder, rng, event),
            ),
            Event::SmallNetwork(event) => reactor::wrap_effects(
                Event::SmallNetwork,
                self.small_network.handle_event(effect_builder, rng, event),
            ),
            Event::BlockProposer(event) => reactor::wrap_effects(
                Event::BlockProposer,
                self.block_proposer.handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::RpcServer(event) => reactor::wrap_effects(
                Event::RpcServer,
                self.rpc_server.handle_event(effect_builder, rng, event),
            ),
            Event::RestServer(event) => reactor::wrap_effects(
                Event::RestServer,
                self.rest_server.handle_event(effect_builder, rng, event),
            ),
            Event::EventStreamServer(event) => reactor::wrap_effects(
                Event::EventStreamServer,
                self.event_stream_server
                    .handle_event(effect_builder, rng, event),
            ),
            Event::ChainspecLoader(event) => reactor::wrap_effects(
                Event::ChainspecLoader,
                self.chainspec_loader
                    .handle_event(effect_builder, rng, event),
            ),
            Event::Consensus(event) => reactor::wrap_effects(
                Event::Consensus,
                self.consensus.handle_event(effect_builder, rng, event),
            ),
            Event::DeployAcceptor(event) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::DeployFetcher(event) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher.handle_event(effect_builder, rng, event),
            ),
            Event::DeployGossiper(event) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::AddressGossiper(event) => reactor::wrap_effects(
                Event::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Into::into,
                self.contract_runtime
                    .handle_event(effect_builder, rng, *event),
            ),
            Event::BlockValidator(event) => reactor::wrap_effects(
                Event::BlockValidator,
                self.block_validator
                    .handle_event(effect_builder, rng, event),
            ),
            Event::LinearChain(event) => reactor::wrap_effects(
                Event::LinearChain,
                self.linear_chain.handle_event(effect_builder, rng, event),
            ),

            // Requests:
            Event::NetworkRequest(req) => {
                let event = if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_ok() {
                    Event::Network(network::Event::from(req))
                } else {
                    Event::SmallNetwork(small_network::Event::from(req))
                };
                self.dispatch_event(effect_builder, rng, event)
            }
            Event::NetworkInfoRequest(req) => {
                let event = if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_ok() {
                    Event::Network(network::Event::from(req))
                } else {
                    Event::SmallNetwork(small_network::Event::from(req))
                };
                self.dispatch_event(effect_builder, rng, event)
            }
            Event::DeployFetcherRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::DeployFetcher(req.into()))
            }
            Event::BlockProposerRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::BlockProposer(req.into()))
            }
            Event::BlockValidatorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::BlockValidator(block_validator::Event::from(req)),
            ),
            Event::MetricsRequest(req) => reactor::wrap_effects(
                Event::MetricsRequest,
                self.metrics.handle_event(effect_builder, rng, req),
            ),
            Event::ChainspecLoaderRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::ChainspecLoader(req.into()))
            }
            Event::StorageRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::Storage(req.into()))
            }
            Event::StateStoreRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::Storage(req.into()))
            }

            // Announcements:
            Event::ControlAnnouncement(ctrl_ann) => {
                unreachable!("unhandled control announcement: {}", ctrl_ann)
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => {
                let reactor_event = match payload {
                    Message::Consensus(msg) => {
                        Event::Consensus(consensus::Event::MessageReceived { sender, msg })
                    }
                    Message::DeployGossiper(message) => {
                        Event::DeployGossiper(gossiper::Event::MessageReceived { sender, message })
                    }
                    Message::AddressGossiper(message) => {
                        Event::AddressGossiper(gossiper::Event::MessageReceived { sender, message })
                    }
                    Message::GetRequest { tag, serialized_id } => {
                        return self.handle_get_request(effect_builder, sender, tag, &serialized_id)
                    }
                    Message::GetResponse {
                        tag,
                        serialized_item,
                    } => match tag {
                        Tag::Deploy => {
                            let deploy: Box<Deploy> = match bincode::deserialize::<
                                FetchedOrNotFound<Deploy, DeployHash>,
                            >(
                                &serialized_item
                            ) {
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
                            Event::DeployAcceptor(deploy_acceptor::Event::Accept {
                                deploy,
                                source: Source::Peer(sender),
                                responder: None,
                            })
                        }
                        Tag::Block => {
                            error!(
                                "cannot handle get response for block-by-hash from {}",
                                sender
                            );
                            return Effects::new();
                        }
                        Tag::BlockAndMetadataByHeight => {
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
                        Tag::Trie => {
                            error!("cannot handle get response for read-trie from {}", sender);
                            return Effects::new();
                        }
                    },
                    Message::FinalitySignature(fs) => {
                        Event::LinearChain(linear_chain::Event::FinalitySignatureReceived(fs, true))
                    }
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(gossiped_address)) => {
                let event = gossiper::Event::ItemReceived {
                    item_id: gossiped_address,
                    source: Source::<NodeId>::Ourself,
                };
                self.dispatch_event(effect_builder, rng, Event::AddressGossiper(event))
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::NewPeer(_peer_id)) => {
                trace!("new peer announcement not handled in the participating reactor");
                Effects::new()
            }
            Event::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived {
                deploy,
                responder,
            }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::<NodeId>::Client,
                    responder,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployAcceptor(event))
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::AcceptedNewDeploy {
                deploy,
                source,
            }) => {
                let event = gossiper::Event::ItemReceived {
                    item_id: *deploy.id(),
                    source: source.clone(),
                };
                let mut effects =
                    self.dispatch_event(effect_builder, rng, Event::DeployGossiper(event));

                let event = event_stream_server::Event::DeployAccepted(*deploy.id());
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::EventStreamServer(event),
                ));

                let event = fetcher::Event::GotRemotely {
                    item: deploy,
                    source,
                };
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::DeployFetcher(event),
                ));

                effects
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                deploy: _,
                source: _,
            }) => Effects::new(),
            Event::ConsensusAnnouncement(consensus_announcement) => match consensus_announcement {
                ConsensusAnnouncement::Finalized(block) => {
                    let reactor_event =
                        Event::BlockProposer(block_proposer::Event::FinalizedBlock(block));
                    self.dispatch_event(effect_builder, rng, reactor_event)
                }
                ConsensusAnnouncement::CreatedFinalitySignature(fs) => self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::LinearChain(linear_chain::Event::FinalitySignatureReceived(fs, false)),
                ),
                ConsensusAnnouncement::Fault {
                    era_id,
                    public_key,
                    timestamp,
                } => {
                    let reactor_event =
                        Event::EventStreamServer(event_stream_server::Event::Fault {
                            era_id,
                            public_key: *public_key,
                            timestamp,
                        });
                    self.dispatch_event(effect_builder, rng, reactor_event)
                }
            },
            Event::ContractRuntimeAnnouncement(ContractRuntimeAnnouncement::LinearChainBlock(
                linear_chain_block,
            )) => {
                let LinearChainBlock {
                    block,
                    execution_results,
                } = *linear_chain_block;
                let mut effects = Effects::new();
                let block_hash = *block.hash();

                // send to linear chain
                let reactor_event = Event::LinearChain(linear_chain::Event::NewLinearChainBlock {
                    block: Box::new(block),
                    execution_results: execution_results
                        .iter()
                        .map(|(hash, (_header, results))| (*hash, results.clone()))
                        .collect(),
                });
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));

                // send to event stream
                for (deploy_hash, (deploy_header, execution_result)) in execution_results {
                    let reactor_event =
                        Event::EventStreamServer(event_stream_server::Event::DeployProcessed {
                            deploy_hash,
                            deploy_header: Box::new(deploy_header),
                            block_hash,
                            execution_result: Box::new(execution_result),
                        });
                    effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                }

                effects
            }
            Event::ContractRuntimeAnnouncement(ContractRuntimeAnnouncement::StepSuccess {
                era_id,
                execution_effect,
            }) => {
                let reactor_event = Event::EventStreamServer(event_stream_server::Event::Step {
                    era_id,
                    execution_effect,
                });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::DeployGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_deploy_id,
            )) => {
                error!(%gossiped_deploy_id, "gossiper should not announce new deploy");
                Effects::new()
            }
            Event::DeployGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(
                gossiped_deploy_id,
            )) => {
                let reactor_event =
                    Event::BlockProposer(block_proposer::Event::BufferDeploy(gossiped_deploy_id));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::AddressGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_address,
            )) => {
                let reactor_event = Event::SmallNetwork(small_network::Event::PeerAddressReceived(
                    gossiped_address,
                ));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::AddressGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(_)) => {
                // We don't care about completion of gossiping an address.
                Effects::new()
            }
            Event::LinearChainAnnouncement(LinearChainAnnouncement::BlockAdded(block)) => {
                let reactor_event_consensus = Event::Consensus(consensus::Event::BlockAdded(
                    Box::new(block.header().clone()),
                ));
                let reactor_event_es =
                    Event::EventStreamServer(event_stream_server::Event::BlockAdded(block.clone()));
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event_es);
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event_consensus));
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    small_network::Event::from(LinearChainAnnouncement::BlockAdded(block)).into(),
                ));
                effects
            }
            Event::LinearChainAnnouncement(LinearChainAnnouncement::NewFinalitySignature(fs)) => {
                let reactor_event =
                    Event::EventStreamServer(event_stream_server::Event::FinalitySignature(fs));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::ChainspecLoaderAnnouncement(
                ChainspecLoaderAnnouncement::UpgradeActivationPointRead(next_upgrade),
            ) => {
                let reactor_event = Event::ChainspecLoader(
                    chainspec_loader::Event::GotNextUpgrade(next_upgrade.clone()),
                );
                let mut effects = self.dispatch_event(effect_builder, rng, reactor_event);

                let reactor_event = Event::Consensus(consensus::Event::GotUpgradeActivationPoint(
                    next_upgrade.activation_point(),
                ));
                effects.extend(self.dispatch_event(effect_builder, rng, reactor_event));
                effects
            }
            Event::BlocklistAnnouncement(ann) => {
                self.dispatch_event(effect_builder, rng, Event::SmallNetwork(ann.into()))
            }
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

#[cfg(test)]
impl NetworkedReactor for Reactor {
    type NodeId = NodeId;
    fn node_id(&self) -> Self::NodeId {
        if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_ok() {
            self.network.node_id()
        } else {
            self.small_network.node_id()
        }
    }
}
