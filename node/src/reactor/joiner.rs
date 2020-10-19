//! Reactor used to join the network.

use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use tracing::{error, info, warn};

use block_executor::BlockExecutor;
use consensus::EraSupervisor;
use deploy_acceptor::DeployAcceptor;
use small_network::GossipedAddress;

use crate::{
    components::{
        block_executor,
        block_validator::{self, BlockValidator},
        chainspec_loader::ChainspecLoader,
        consensus::{self},
        contract_runtime::{self, ContractRuntime},
        deploy_acceptor,
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        linear_chain,
        linear_chain_sync::{self, LinearChainSync},
        small_network::{self, NodeId, SmallNetwork},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            BlockExecutorAnnouncement, ConsensusAnnouncement, DeployAcceptorAnnouncement,
            GossiperAnnouncement, LinearChainAnnouncement, NetworkAnnouncement,
        },
        requests::{
            BlockExecutorRequest, BlockValidationRequest, ConsensusRequest, ContractRuntimeRequest,
            DeployBufferRequest, FetcherRequest, LinearChainRequest, NetworkRequest,
            StorageRequest,
        },
        EffectBuilder, Effects,
    },
    protocol::Message,
    reactor::{
        self,
        event_queue_metrics::EventQueueMetrics,
        initializer,
        validator::{self, Error, ValidatorInitConfig},
        EventQueueHandle, Finalize,
    },
    types::{Block, BlockByHeight, BlockHeader, CryptoRngCore, Deploy, ProtoBlock, Tag, Timestamp},
    utils::{Source, WithDir},
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

    /// Linear chain (by height) fetcher event.
    #[from]
    BlockByHeightFetcher(fetcher::Event<BlockByHeight>),

    /// Deploy fetcher event.
    #[from]
    DeployFetcher(fetcher::Event<Deploy>),

    /// Deploy acceptor event.
    #[from]
    DeployAcceptor(deploy_acceptor::Event),

    /// Block validator event.
    #[from]
    BlockValidator(block_validator::Event<BlockHeader, NodeId>),

    /// Linear chain event.
    #[from]
    LinearChainSync(linear_chain_sync::Event<NodeId>),

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

    /// Address gossiper event.
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),

    /// Requests.
    /// Linear chain block by hash fetcher request.
    #[from]
    BlockFetcherRequest(FetcherRequest<NodeId, Block>),

    /// Linear chain block by height fetcher request.
    #[from]
    BlockByHeightFetcherRequest(FetcherRequest<NodeId, BlockByHeight>),

    /// Deploy fetcher request.
    #[from]
    DeployFetcherRequest(FetcherRequest<NodeId, Deploy>),

    /// Block validation request.
    #[from]
    BlockValidatorRequest(BlockValidationRequest<BlockHeader, NodeId>),

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

    /// Address Gossiper announcement.
    #[from]
    AddressGossiperAnnouncement(GossiperAnnouncement<GossipedAddress>),

    /// DeployAcceptor announcement.
    #[from]
    DeployAcceptorAnnouncement(DeployAcceptorAnnouncement<NodeId>),

    /// Linear chain announcement.
    #[from]
    LinearChainAnnouncement(LinearChainAnnouncement),
}

impl From<LinearChainRequest<NodeId>> for Event {
    fn from(req: LinearChainRequest<NodeId>) -> Self {
        Event::LinearChain(linear_chain::Event::Request(req))
    }
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

impl From<NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>> for Event {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>) -> Self {
        Event::Network(small_network::Event::from(
            request.map_payload(Message::from),
        ))
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
            Event::BlockByHeightFetcherRequest(request) => {
                write!(f, "block by height fetcher request: {}", request)
            }
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
            Event::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            Event::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
            Event::BlockByHeightFetcher(event) => {
                write!(f, "block by height fetcher event: {}", event)
            }
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            Event::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            Event::LinearChainAnnouncement(ann) => write!(f, "linear chain announcement: {}", ann),
        }
    }
}

/// Joining node reactor.
#[derive(DataSize)]
pub struct Reactor {
    pub(super) net: SmallNetwork<Event, Message>,
    pub(super) address_gossiper: Gossiper<GossipedAddress, Event>,
    pub(super) config: validator::Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
    pub(super) linear_chain_fetcher: Fetcher<Block>,
    pub(super) linear_chain_sync: LinearChainSync<NodeId>,
    pub(super) block_validator: BlockValidator<BlockHeader, NodeId>,
    pub(super) deploy_fetcher: Fetcher<Deploy>,
    pub(super) block_executor: BlockExecutor,
    pub(super) linear_chain: linear_chain::LinearChain<NodeId>,
    pub(super) consensus: EraSupervisor<NodeId>,
    // Effects consensus component returned during creation.
    // In the `joining` phase we don't want to handle it,
    // so we carry them forward to the `validator` reactor.
    #[data_size(skip)]
    // Unfortunately, we have no way of inspecting the future and its heap allocations at all.
    pub(super) init_consensus_effects: Effects<consensus::Event<NodeId>>,
    // Handles request for linear chain block by height.
    pub(super) block_by_height_fetcher: Fetcher<BlockByHeight>,
    #[data_size(skip)]
    pub(super) deploy_acceptor: DeployAcceptor,
    #[data_size(skip)]
    event_queue_metrics: EventQueueMetrics,
}

impl reactor::Reactor for Reactor {
    type Event = Event;

    // The "configuration" is in fact the whole state of the initializer reactor, which we
    // deconstruct and reuse.
    type Config = WithDir<initializer::Reactor>;
    type Error = Error;

    fn new(
        initializer: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (root, initializer) = initializer.into_parts();

        let initializer::Reactor {
            config,
            chainspec_loader,
            storage,
            contract_runtime,
        } = initializer;

        let event_queue_metrics = EventQueueMetrics::new(registry.clone(), event_queue)?;

        let (net, net_effects) = SmallNetwork::new(event_queue, config.network.clone(), false)?;

        let linear_chain_fetcher = Fetcher::new(config.gossip);
        let effects = reactor::wrap_effects(Event::Network, net_effects);

        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", config.gossip, registry)?;

        let effect_builder = EffectBuilder::new(event_queue);

        let init_hash = config.node.trusted_hash;

        match init_hash {
            None => info!("No synchronization of the linear chain will be done."),
            Some(hash) => info!("Synchronizing linear chain from: {:?}", hash),
        }

        let linear_chain_sync = LinearChainSync::new(init_hash);

        let block_validator = BlockValidator::new();

        let deploy_fetcher = Fetcher::new(config.gossip);

        let block_by_height_fetcher = Fetcher::new(config.gossip);

        let deploy_acceptor = DeployAcceptor::new();

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
            WithDir::new(root, config.consensus.clone()),
            effect_builder,
            validator_stakes,
            chainspec_loader.chainspec(),
            chainspec_loader
                .genesis_post_state_hash()
                .expect("should have genesis post state hash"),
            rng,
        )?;

        Ok((
            Self {
                net,
                address_gossiper,
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
                block_by_height_fetcher,
                deploy_acceptor,
                event_queue_metrics,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut dyn CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.net.handle_event(effect_builder, rng, event),
            ),
            Event::NetworkAnnouncement(NetworkAnnouncement::NewPeer(id)) => reactor::wrap_effects(
                Event::LinearChainSync,
                self.linear_chain_sync.handle_event(
                    effect_builder,
                    rng,
                    linear_chain_sync::Event::NewPeerConnected(id),
                ),
            ),
            Event::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(gossiped_address)) => {
                let event = gossiper::Event::ItemReceived {
                    item_id: gossiped_address,
                    source: Source::<NodeId>::Client,
                };
                self.dispatch_event(effect_builder, rng, Event::AddressGossiper(event))
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => match payload {
                Message::GetResponse {
                    tag: Tag::Block,
                    serialized_item,
                } => {
                    let block = match bincode::deserialize(&serialized_item) {
                        Ok(block) => Box::new(block),
                        Err(err) => {
                            error!("failed to decode block from {}: {}", sender, err);
                            return Effects::new();
                        }
                    };
                    let event = fetcher::Event::GotRemotely {
                        item: block,
                        source: Source::Peer(sender),
                    };
                    self.dispatch_event(effect_builder, rng, Event::BlockFetcher(event))
                }
                Message::GetResponse {
                    tag: Tag::BlockByHeight,
                    serialized_item,
                } => {
                    let block_at_height: BlockByHeight =
                        match bincode::deserialize(&serialized_item) {
                            Ok(maybe_block) => maybe_block,
                            Err(err) => {
                                error!("failed to decode block from {}: {}", sender, err);
                                return Effects::new();
                            }
                        };

                    let event = match block_at_height {
                        BlockByHeight::Absent(block_height) => fetcher::Event::AbsentRemotely {
                            id: block_height,
                            peer: sender,
                        },
                        BlockByHeight::Block(block) => fetcher::Event::GotRemotely {
                            item: Box::new(BlockByHeight::Block(block)),
                            source: Source::Peer(sender),
                        },
                    };
                    self.dispatch_event(effect_builder, rng, Event::BlockByHeightFetcher(event))
                }
                Message::GetResponse {
                    tag: Tag::Deploy,
                    serialized_item,
                } => {
                    let deploy = match bincode::deserialize(&serialized_item) {
                        Ok(deploy) => Box::new(deploy),
                        Err(err) => {
                            error!("failed to decode deploy from {}: {}", sender, err);
                            return Effects::new();
                        }
                    };
                    let event = Event::DeployAcceptor(deploy_acceptor::Event::Accept {
                        deploy,
                        source: Source::Peer(sender),
                    });
                    self.dispatch_event(effect_builder, rng, event)
                }
                // needed so that consensus can notify us of the eras it knows of
                // TODO: remove when proper syncing is implemented
                Message::Consensus(msg) => self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::Consensus(consensus::Event::MessageReceived { sender, msg }),
                ),
                Message::AddressGossiper(message) => {
                    let event = Event::AddressGossiper(gossiper::Event::MessageReceived {
                        sender,
                        message,
                    });
                    self.dispatch_event(effect_builder, rng, event)
                }
                other => {
                    warn!(?other, "network announcement ignored.");
                    Effects::new()
                }
            },
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::AcceptedNewDeploy {
                deploy,
                source,
            }) => {
                let event = fetcher::Event::GotRemotely {
                    item: deploy,
                    source,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployFetcher(event))
            }
            Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                deploy,
                source,
            }) => {
                let deploy_hash = *deploy.id();
                let peer = source;
                warn!(?deploy_hash, ?peer, "Invalid deploy received from a peer.");
                Effects::new()
            }
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
            Event::DeployAcceptor(event) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
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
            Event::BlockByHeightFetcher(event) => reactor::wrap_effects(
                Event::BlockByHeightFetcher,
                self.block_by_height_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            Event::DeployFetcherRequest(request) => {
                self.dispatch_event(effect_builder, rng, Event::DeployFetcher(request.into()))
            }
            Event::BlockByHeightFetcherRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                Event::BlockByHeightFetcher(request.into()),
            ),
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
            Event::BlockExecutorAnnouncement(BlockExecutorAnnouncement::LinearChainBlock {
                block,
                execution_results,
            }) => {
                let reactor_event = Event::LinearChain(linear_chain::Event::LinearChainBlock {
                    block: Box::new(block),
                    execution_results,
                });
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
            Event::ConsensusAnnouncement(announcement) => match announcement {
                ConsensusAnnouncement::Handled(block_header) => reactor::wrap_effects(
                    Event::LinearChainSync,
                    self.linear_chain_sync.handle_event(
                        effect_builder,
                        rng,
                        linear_chain_sync::Event::BlockHandled(block_header),
                    ),
                ),
                other => {
                    warn!("Ignoring consensus announcement {}", other);
                    Effects::new()
                }
            },
            Event::DeployBufferRequest(request) => {
                // Consensus component should not be trying to create new blocks during joining
                // phase.
                error!("Ignoring deploy buffer request {}", request);
                Effects::new()
            }
            Event::ProtoBlockValidatorRequest(request) => {
                // During joining phase, consensus component should not be requesting
                // validation of the proto block.
                error!("Ignoring proto block validation request {}", request);
                Effects::new()
            }
            Event::AddressGossiper(event) => reactor::wrap_effects(
                Event::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::AddressGossiperAnnouncement(ann) => {
                let GossiperAnnouncement::NewCompleteItem(gossiped_address) = ann;
                let reactor_event =
                    Event::Network(small_network::Event::PeerAddressReceived(gossiped_address));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::LinearChainAnnouncement(ann) => {
                warn!("Ignoring linear chain announcement {}", ann);
                Effects::new()
            }
        }
    }

    fn is_stopped(&mut self) -> bool {
        self.linear_chain_sync.is_synced()
    }

    fn update_metrics(&mut self, event_queue_handle: EventQueueHandle<Self::Event>) {
        self.event_queue_metrics
            .record_event_queue_counts(&event_queue_handle)
    }
}

impl Reactor {
    /// Deconstructs the reactor into config useful for creating a Validator reactor. Shuts down
    /// the network, closing all incoming and outgoing connections, and frees up the listening
    /// socket.
    pub async fn into_validator_config(self) -> ValidatorInitConfig {
        let linear_chain = self.linear_chain.linear_chain();
        let finalized_deploys = self.storage.get_finalized_deploys(linear_chain).await;
        let (net, config) = (
            self.net,
            ValidatorInitConfig {
                chainspec_loader: self.chainspec_loader,
                config: self.config,
                contract_runtime: self.contract_runtime,
                storage: self.storage,
                consensus: self.consensus,
                init_consensus_effects: self.init_consensus_effects,
                linear_chain: linear_chain.clone(),
                finalized_deploys,
            },
        );
        net.finalize().await;
        config
    }
}
