//! Reactor used to join the network.

use std::fmt::{self, Display, Formatter};

use block_executor::BlockExecutor;
use consensus::EraSupervisor;
use derive_more::From;
use prometheus::Registry;
use rand::{CryptoRng, Rng};
use tracing::warn;

use crate::{
    components::{
        block_executor,
        block_validator::{self, BlockValidator},
        chainspec_loader::ChainspecLoader,
        consensus,
        contract_runtime::{self, ContractRuntime},
        fetcher::{self, Fetcher},
        linear_chain,
        linear_chain_sync::{self, LinearChainSync},
        small_network::{self, NodeId, SmallNetwork},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{BlockExecutorAnnouncement, ConsensusAnnouncement, NetworkAnnouncement},
        requests::{
            BlockExecutorRequest, BlockValidationRequest, ConsensusRequest, ContractRuntimeRequest,
            DeployBufferRequest, FetcherRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, Effects,
    },
    protocol::Message,
    reactor::{
        self, initializer,
        validator::{self, Error, ValidatorInitConfig},
        EventQueueHandle, Finalize,
    },
    types::{Block, Deploy, ProtoBlock, Timestamp},
    utils::WithDir,
};

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
pub enum Event {
    /// Network event.
    #[from]
    Network(small_network::Event<Message>),

    /// Storage event.
    #[from]
    Storage(storage::Event<Storage>),

    /// Linear chain fetcher event.
    #[from]
    BlockFetcher(fetcher::Event<Block>),

    /// Deploy fetcher event.
    #[from]
    DeployFetcher(fetcher::Event<Deploy>),

    /// Block validator event.
    #[from]
    BlockValidator(block_validator::Event<Block, NodeId>),

    /// Linear chain event.
    #[from]
    LinearChainSync(linear_chain_sync::Event),

    /// Block executor event.
    #[from]
    BlockExecutor(block_executor::Event),

    /// Contract Runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),

    /// Linear chain event.
    #[from]
    LinearChain(linear_chain::Event<NodeId>),

    /// Consensus component event.
    #[from]
    Consensus(consensus::Event<NodeId>),

    /// Requests.
    /// Linear chain fetcher request.
    #[from]
    BlockFetcherRequest(FetcherRequest<NodeId, Block>),

    /// Deploy fetcher request.
    #[from]
    DeployFetcherRequest(FetcherRequest<NodeId, Deploy>),

    /// Block validation request.
    #[from]
    BlockValidatorRequest(BlockValidationRequest<Block, NodeId>),

    /// Block executor request.
    #[from]
    BlockExecutorRequest(BlockExecutorRequest),

    /// Deploy buffer request.
    #[from]
    DeployBufferRequest(DeployBufferRequest),

    /// Proto block validator request.
    #[from]
    ProtoBlockValidatorRequest(BlockValidationRequest<ProtoBlock, NodeId>),

    // Announcements
    /// Network announcement.
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),

    /// Block executor annoncement.
    #[from]
    BlockExecutorAnnouncement(BlockExecutorAnnouncement),

    /// Consensus announcement.
    #[from]
    ConsensusAnnouncement(ConsensusAnnouncement),
}

impl From<StorageRequest<Storage>> for Event {
    fn from(request: StorageRequest<Storage>) -> Self {
        Event::Storage(storage::Event::Request(request))
    }
}

impl From<NetworkRequest<NodeId, Message>> for Event {
    fn from(request: NetworkRequest<NodeId, Message>) -> Self {
        Event::Network(small_network::Event::from(request))
    }
}

impl From<ContractRuntimeRequest> for Event {
    fn from(request: ContractRuntimeRequest) -> Event {
        Event::ContractRuntime(contract_runtime::Event::Request(request))
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
            Event::NetworkAnnouncement(event) => write!(f, "network announcement: {}", event),
            Event::Storage(request) => write!(f, "storage: {}", request),
            Event::BlockFetcherRequest(request) => write!(f, "block fetcher request: {}", request),
            Event::BlockValidatorRequest(request) => {
                write!(f, "block validator request: {}", request)
            }
            Event::DeployFetcherRequest(request) => {
                write!(f, "deploy fetcher request: {}", request)
            }
            Event::LinearChainSync(event) => write!(f, "linear chain: {}", event),
            Event::BlockFetcher(event) => write!(f, "block fetcher: {}", event),
            Event::BlockValidator(event) => write!(f, "block validator event: {}", event),
            Event::DeployFetcher(event) => write!(f, "deploy fetcher event: {}", event),
            Event::BlockExecutor(event) => write!(f, "block executor event: {}", event),
            Event::BlockExecutorRequest(request) => {
                write!(f, "block executor request: {}", request)
            }
            Event::DeployBufferRequest(req) => write!(f, "deploy buffer request: {}", req),
            Event::ContractRuntime(event) => write!(f, "contract runtime event: {}", event),
            Event::LinearChain(event) => write!(f, "linear chain event: {}", event),
            Event::BlockExecutorAnnouncement(announcement) => {
                write!(f, "block executor announcement: {}", announcement)
            }
            Event::Consensus(event) => write!(f, "consensus event: {}", event),
            Event::ConsensusAnnouncement(ann) => write!(f, "consensus announcement: {}", ann),
            Event::ProtoBlockValidatorRequest(req) => write!(f, "block validator request: {}", req),
        }
    }
}

/// Joining node reactor.
pub struct Reactor<R: Rng + CryptoRng + ?Sized> {
    pub(super) net: SmallNetwork<Event, Message>,
    pub(super) config: validator::Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
    pub(super) linear_chain_fetcher: Fetcher<Block>,
    pub(super) linear_chain_sync: LinearChainSync<NodeId>,
    pub(super) block_validator: BlockValidator<Block, NodeId>,
    pub(super) deploy_fetcher: Fetcher<Deploy>,
    pub(super) block_executor: BlockExecutor,
    pub(super) linear_chain: linear_chain::LinearChain<NodeId>,
    pub(super) consensus: EraSupervisor<NodeId, R>,
    // Effects consensus component returned during creation.
    // In the `joining` phase we don't want to handle it,
    // so we carry them forward to the `validator` reactor.
    pub(super) init_consensus_effects: Effects<consensus::Event<NodeId>>,
}

impl<R: Rng + CryptoRng + ?Sized> reactor::Reactor<R> for Reactor<R> {
    type Event = Event;

    // The "configuration" is in fact the whole state of the initializer reactor, which we
    // deconstruct and reuse.
    type Config = WithDir<initializer::Reactor>;
    type Error = Error;

    fn new(
        initializer: Self::Config,
        _registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut R,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (root, initializer) = initializer.into_parts();

        let initializer::Reactor {
            config,
            chainspec_loader,
            storage,
            contract_runtime,
        } = initializer;

        let (net, net_effects) = SmallNetwork::new(event_queue, config.network.clone())?;

        let linear_chain_fetcher = Fetcher::new(config.gossip);

        let mut effects = reactor::wrap_effects(Event::Network, net_effects);

        let effect_builder = EffectBuilder::new(event_queue);

        let (linear_chain_sync, linear_effects) =
            LinearChainSync::new(Vec::new(), effect_builder, config.node.trusted_hash);

        effects.extend(reactor::wrap_effects(
            Event::LinearChainSync,
            linear_effects,
        ));

        let block_validator = BlockValidator::new();

        let deploy_fetcher = Fetcher::new(config.gossip);

        let genesis_post_state_hash = chainspec_loader
            .genesis_post_state_hash()
            .expect("Should have Genesis post state hash");

        let block_executor = BlockExecutor::new(genesis_post_state_hash);

        let linear_chain = linear_chain::LinearChain::new();

        let validator_stakes = chainspec_loader
            .chainspec()
            .genesis
            .genesis_validator_stakes();

        // Used to decide whether era should be activated.
        let timestamp = Timestamp::now();

        let (consensus, init_consensus_effects) = EraSupervisor::new(
            timestamp,
            WithDir::new(root.clone(), config.consensus.clone()),
            effect_builder,
            validator_stakes,
            &chainspec_loader.chainspec().genesis.highway_config,
            rng,
        )?;

        Ok((
            Self {
                net,
                config,
                chainspec_loader,
                storage,
                contract_runtime,
                linear_chain_sync,
                linear_chain_fetcher,
                block_validator,
                deploy_fetcher,
                block_executor,
                linear_chain,
                consensus,
                init_consensus_effects,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.net.handle_event(effect_builder, rng, event),
            ),
            Event::NetworkAnnouncement(_) => Default::default(),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::BlockFetcherRequest(request) => {
                self.dispatch_event(effect_builder, rng, Event::BlockFetcher(request.into()))
            }

            Event::BlockValidatorRequest(request) => {
                self.dispatch_event(effect_builder, rng, Event::BlockValidator(request.into()))
            }
            Event::LinearChainSync(event) => reactor::wrap_effects(
                Event::LinearChainSync,
                self.linear_chain_sync
                    .handle_event(effect_builder, rng, event),
            ),
            Event::BlockFetcher(event) => reactor::wrap_effects(
                Event::BlockFetcher,
                self.linear_chain_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            Event::BlockValidator(event) => reactor::wrap_effects(
                Event::BlockValidator,
                self.block_validator
                    .handle_event(effect_builder, rng, event),
            ),
            Event::DeployFetcher(event) => reactor::wrap_effects(
                Event::DeployFetcher,
                self.deploy_fetcher.handle_event(effect_builder, rng, event),
            ),
            Event::DeployFetcherRequest(request) => {
                self.dispatch_event(effect_builder, rng, Event::DeployFetcher(request.into()))
            }
            Event::BlockExecutor(event) => reactor::wrap_effects(
                Event::BlockExecutor,
                self.block_executor.handle_event(effect_builder, rng, event),
            ),
            Event::BlockExecutorRequest(request) => {
                self.dispatch_event(effect_builder, rng, Event::BlockExecutor(request.into()))
            }
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            Event::BlockExecutorAnnouncement(BlockExecutorAnnouncement::LinearChainBlock(
                block,
            )) => {
                let reactor_event =
                    Event::LinearChain(linear_chain::Event::LinearChainBlock(block));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::LinearChain(event) => reactor::wrap_effects(
                Event::LinearChain,
                self.linear_chain.handle_event(effect_builder, rng, event),
            ),
            Event::Consensus(event) => reactor::wrap_effects(
                Event::Consensus,
                self.consensus.handle_event(effect_builder, rng, event),
            ),
            Event::ConsensusAnnouncement(announcement) => {
                // During joining phase we don't need to act on consensus announcements.
                warn!("Ignoring consensus announcement {}", announcement);
                Effects::new()
            }
            Event::DeployBufferRequest(request) => {
                // Consensus component should not be trying to create new blocks during joining
                // phase.
                warn!("Ignoring deploy buffer request {}", request);
                Effects::new()
            }
            Event::ProtoBlockValidatorRequest(request) => {
                // During joining phase, consensus component should not be requesting
                // validation of the proto block.
                warn!("Ignoring proto block validation request {}", request);
                Effects::new()
            }
        }
    }

    fn is_stopped(&mut self) -> bool {
        self.linear_chain_sync.is_synced()
    }
}

impl<R: Rng + CryptoRng + ?Sized> Reactor<R> {
    /// Deconstructs the reactor into config useful for creating a Validator reactor. Shuts down
    /// the network, closing all incoming and outgoing connections, and frees up the listening
    /// socket.
    pub async fn into_validator_config(self) -> ValidatorInitConfig<R> {
        let (net, config) = (
            self.net,
            ValidatorInitConfig {
                chainspec_loader: self.chainspec_loader,
                config: self.config,
                contract_runtime: self.contract_runtime,
                storage: self.storage,
                consensus: self.consensus,
                init_consensus_effects: self.init_consensus_effects,
            },
        );
        net.finalize().await;
        config
    }
}
