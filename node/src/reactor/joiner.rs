//! Reactor used to join the network.

use std::fmt::{self, Display, Formatter};

use block_executor::BlockExecutor;
use consensus::EraSupervisor;
use deploy_acceptor::DeployAcceptor;
use deploy_buffer::DeployBuffer;
use derive_more::From;
use prometheus::Registry;
use rand::{CryptoRng, Rng};
use small_network::GossipedAddress;
use tracing::{error, info, trace, warn};

use crate::{
    components::{
        block_executor,
        block_validator::{self, BlockValidator},
        chainspec_loader::ChainspecLoader,
        consensus::{self, EraId},
        contract_runtime::{self, ContractRuntime},
        deploy_acceptor, deploy_buffer,
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        linear_chain,
        linear_chain_sync::{self, LinearChainSync},
        small_network::{self, NodeId, SmallNetwork},
        storage::{self, Storage},
        Component,
    },
    crypto::hash::Digest,
    effect::{
        announcements::{
            BlockExecutorAnnouncement, ConsensusAnnouncement, DeployAcceptorAnnouncement,
            GossiperAnnouncement, NetworkAnnouncement,
        },
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
    types::{Block, BlockHash, Deploy, ProtoBlock, Tag, Timestamp},
    utils::{Source, WithDir},
};

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
pub enum Event {
    /// Network event.
    #[from]
    Network(small_network::Event<Message>),

    /// Deploy buffer event.
    #[from]
    DeployBuffer(deploy_buffer::Event),

    /// Storage event.
    #[from]
    Storage(storage::Event<Storage>),

    /// Linear chain fetcher event.
    #[from]
    BlockFetcher(fetcher::Event<Block>),

    /// Deploy fetcher event.
    #[from]
    DeployFetcher(fetcher::Event<Deploy>),

    /// Deploy acceptor event.
    #[from]
    DeployAcceptor(deploy_acceptor::Event),

    /// Block validator event.
    #[from]
    BlockValidator(block_validator::Event<Block, NodeId>),

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

    /// Address Gossiper announcement.
    #[from]
    AddressGossiperAnnouncement(GossiperAnnouncement<GossipedAddress>),

    /// DeployAcceptor announcement.
    #[from]
    DeployAcceptorAnnouncement(DeployAcceptorAnnouncement<NodeId>),
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
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            Event::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            Event::DeployBuffer(event) => write!(f, "deploy buffer: {}", event),
        }
    }
}

/// Joining node reactor.
pub struct Reactor<R: Rng + CryptoRng + ?Sized> {
    pub(super) net: SmallNetwork<Event, Message>,
    pub(super) address_gossiper: Gossiper<GossipedAddress, Event>,
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
    // Handles request for linear chain block by height.
    pub(super) deploy_acceptor: DeployAcceptor,
    pub(super) deploy_buffer: DeployBuffer,
    // TODO: remove after proper syncing is implemented
    // `None` means we haven't seen any consensus messages yet
    latest_received_era_id: Option<EraId>,
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
        let effects = reactor::wrap_effects(Event::Network, net_effects);

        let address_gossiper = Gossiper::new_for_complete_items(config.gossip);

        let effect_builder = EffectBuilder::new(event_queue);

        let init_hash = config
            .node
            .trusted_hash
            .clone()
            .map(|str_hash| BlockHash::new(Digest::from_hex(str_hash).unwrap()));

        match init_hash {
            None => info!("No synchronization of the linear chain will be done."),
            Some(hash) => info!("Synchronizing linear chain from: {:?}", hash),
        }

        let linear_chain_sync = LinearChainSync::new(effect_builder, init_hash);

        let block_validator = BlockValidator::new();

        let deploy_fetcher = Fetcher::new(config.gossip);

        let deploy_acceptor = DeployAcceptor::new();

        let deploy_buffer = DeployBuffer::new(config.node.block_max_deploy_count as usize);

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
                deploy_acceptor,
                deploy_buffer,
                latest_received_era_id: None,
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
            Event::DeployBuffer(event) => reactor::wrap_effects(
                Event::DeployBuffer,
                self.deploy_buffer.handle_event(effect_builder, rng, event),
            ),
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
                    let block = match rmp_serde::from_read_ref(&serialized_item) {
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
                // needed so that consensus can notify us of the eras it knows of
                // TODO: remove when proper syncing is implemented
                Message::Consensus(msg) => self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::Consensus(consensus::Event::MessageReceived { sender, msg }),
                ),
                Message::GetResponse {
                    tag: Tag::Deploy,
                    serialized_item,
                } => {
                    let deploy = match rmp_serde::from_read_ref(&serialized_item) {
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
                let event = deploy_buffer::Event::Buffer {
                    hash: *deploy.id(),
                    header: Box::new(deploy.header().clone()),
                };
                let mut effects =
                    self.dispatch_event(effect_builder, rng, Event::DeployBuffer(event));

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
            Event::ConsensusAnnouncement(announcement) => match announcement {
                ConsensusAnnouncement::Handled(height) => reactor::wrap_effects(
                    Event::LinearChainSync,
                    self.linear_chain_sync.handle_event(
                        effect_builder,
                        rng,
                        linear_chain_sync::Event::BlockHandled(height),
                    ),
                ),
                ConsensusAnnouncement::GotMessageInEra(era_id) => {
                    // note if the era id is later than the latest we've received so far
                    if self
                        .latest_received_era_id
                        .map(|lreid| era_id > lreid)
                        .unwrap_or(true)
                    {
                        self.latest_received_era_id = Some(era_id);
                    }
                    Effects::new()
                }
                other => {
                    warn!("Ignoring consensus announcement {}", other);
                    Effects::new()
                }
            },
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
        }
    }

    fn is_stopped(&mut self) -> bool {
        if self.linear_chain_sync.is_synced() {
            trace!("Linear chain is synced.");
            if let (Some(latest_known_era), Some(latest_received_era)) = (
                self.linear_chain_sync.init_block_era(),
                self.latest_received_era_id,
            ) {
                trace!("Latest known era: {:?}", latest_known_era);
                trace!("Latest received era: {:?}", latest_received_era);
                if latest_known_era < latest_received_era {
                    // We only synchronized up to era N, and the other validators are at least at
                    // era N+1 - we won't be able to catch up, so we crash
                    // TODO: remove this once proper syncing is implemented
                    error!(
                        "We are now synchronized up to era {:?}. Unfortunately, while we were \
                        synchronizing, the other validators progressed to era {:?}. Since they \
                        are ahead of us, we won't be able to participate. Try to restart the node \
                        with a later trusted block hash.",
                        latest_known_era, latest_received_era
                    );
                    panic!("other validators ahead, won't be able to participate - please restart");
                }
            } else {
                trace!("No latest known era or latest received era!");
            }
        }
        // We want to wait for a consensus message in order to determine the current consensus era,
        // but only if the trusted hash has been specified
        self.linear_chain_sync.is_synced()
            && (self.latest_received_era_id.is_some()
                || self.linear_chain_sync.init_block_era().is_none())
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
                linear_chain: self.linear_chain.linear_chain().clone(),
            },
        );
        net.finalize().await;
        config
    }
}
