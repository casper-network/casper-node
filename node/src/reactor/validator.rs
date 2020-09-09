//! Reactor for validator nodes.
//!
//! Validator nodes join the validator-only network upon startup.

mod config;
mod error;
#[cfg(test)]
mod tests;

use std::{
    convert::TryInto,
    fmt::{self, Display, Formatter},
    path::PathBuf,
};

use derive_more::From;
use prometheus::Registry;
use rand::{CryptoRng, Rng};
use tracing::{error, warn};

#[cfg(test)]
use crate::testing::network::NetworkedReactor;
use crate::{
    components::{
        api_server::{self, ApiServer},
        block_executor::{self, BlockExecutor},
        block_validator::{self, BlockValidator},
        chainspec_loader::ChainspecLoader,
        consensus::{self, EraSupervisor},
        contract_runtime::{self, ContractRuntime},
        deploy_acceptor::{self, DeployAcceptor},
        deploy_buffer::{self, DeployBuffer},
        fetcher::{self, Fetcher},
        gossiper::{self, Gossiper},
        linear_chain,
        metrics::Metrics,
        small_network::{self, GossipedAddress, NodeId, SmallNetwork},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            ApiServerAnnouncement, BlockExecutorAnnouncement, ConsensusAnnouncement,
            DeployAcceptorAnnouncement, GossiperAnnouncement, NetworkAnnouncement,
        },
        requests::{
            ApiRequest, BlockExecutorRequest, BlockValidationRequest, ConsensusRequest,
            ContractRuntimeRequest, DeployBufferRequest, FetcherRequest, LinearChainRequest,
            MetricsRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, Effects,
    },
    protocol::Message,
    reactor::{self, EventQueueHandle},
    types::{Deploy, ProtoBlock, Tag, Timestamp},
    utils::{Source, WithDir},
};
pub use config::Config;
pub use error::Error;
use linear_chain::LinearChain;

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
    #[from]
    /// Storage event.
    Storage(storage::Event<Storage>),
    #[from]
    /// API server event.
    ApiServer(api_server::Event),
    #[from]
    /// Consensus event.
    Consensus(consensus::Event<NodeId>),
    /// Deploy acceptor event.
    #[from]
    DeployAcceptor(deploy_acceptor::Event),
    /// Deploy fetcher event.
    #[from]
    DeployFetcher(fetcher::Event<Deploy>),
    /// Deploy gossiper event.
    #[from]
    DeployGossiper(gossiper::Event<Deploy>),
    /// Deploy gossiper event.
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),
    /// Contract runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),
    /// Block executor event.
    #[from]
    BlockExecutor(block_executor::Event),
    /// Block validator event.
    #[from]
    ProtoBlockValidator(block_validator::Event<ProtoBlock, NodeId>),
    /// Linear chain event.
    #[from]
    LinearChain(linear_chain::Event<NodeId>),

    // Requests
    /// Network request.
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),
    /// Deploy fetcher request.
    #[from]
    DeployFetcherRequest(FetcherRequest<NodeId, Deploy>),
    /// Deploy buffer request.
    #[from]
    DeployBufferRequest(DeployBufferRequest),
    /// Block executor request.
    #[from]
    BlockExecutorRequest(BlockExecutorRequest),
    /// Block validator request.
    #[from]
    ProtoBlockValidatorRequest(BlockValidationRequest<ProtoBlock, NodeId>),
    /// Metrics request.
    #[from]
    MetricsRequest(MetricsRequest),

    // Announcements
    /// Network announcement.
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
    /// API server announcement.
    #[from]
    ApiServerAnnouncement(ApiServerAnnouncement),
    /// DeployAcceptor announcement.
    #[from]
    DeployAcceptorAnnouncement(DeployAcceptorAnnouncement<NodeId>),
    /// Consensus announcement.
    #[from]
    ConsensusAnnouncement(ConsensusAnnouncement),
    /// BlockExecutor announcement.
    #[from]
    BlockExecutorAnnouncement(BlockExecutorAnnouncement),
    /// Deploy Gossiper announcement.
    #[from]
    DeployGossiperAnnouncement(GossiperAnnouncement<Deploy>),
    /// Address Gossiper announcement.
    #[from]
    AddressGossiperAnnouncement(GossiperAnnouncement<GossipedAddress>),
}

impl From<StorageRequest<Storage>> for Event {
    fn from(request: StorageRequest<Storage>) -> Self {
        Event::Storage(storage::Event::Request(request))
    }
}

impl From<ApiRequest> for Event {
    fn from(request: ApiRequest) -> Self {
        Event::ApiServer(api_server::Event::ApiRequest(request))
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

impl From<LinearChainRequest<NodeId>> for Event {
    fn from(request: LinearChainRequest<NodeId>) -> Self {
        Event::LinearChain(linear_chain::Event::Request(request))
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(f, "network: {}", event),
            Event::DeployBuffer(event) => write!(f, "deploy buffer: {}", event),
            Event::Storage(event) => write!(f, "storage: {}", event),
            Event::ApiServer(event) => write!(f, "api server: {}", event),
            Event::Consensus(event) => write!(f, "consensus: {}", event),
            Event::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            Event::DeployFetcher(event) => write!(f, "deploy fetcher: {}", event),
            Event::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            Event::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            Event::ContractRuntime(event) => write!(f, "contract runtime: {}", event),
            Event::BlockExecutor(event) => write!(f, "block executor: {}", event),
            Event::LinearChain(event) => write!(f, "linear-chain event {}", event),
            Event::ProtoBlockValidator(event) => write!(f, "block validator: {}", event),
            Event::NetworkRequest(req) => write!(f, "network request: {}", req),
            Event::DeployFetcherRequest(req) => write!(f, "deploy fetcher request: {}", req),
            Event::DeployBufferRequest(req) => write!(f, "deploy buffer request: {}", req),
            Event::BlockExecutorRequest(req) => write!(f, "block executor request: {}", req),
            Event::ProtoBlockValidatorRequest(req) => write!(f, "block validator request: {}", req),
            Event::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            Event::NetworkAnnouncement(ann) => write!(f, "network announcement: {}", ann),
            Event::ApiServerAnnouncement(ann) => write!(f, "api server announcement: {}", ann),
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            Event::ConsensusAnnouncement(ann) => write!(f, "consensus announcement: {}", ann),
            Event::BlockExecutorAnnouncement(ann) => {
                write!(f, "block-executor announcement: {}", ann)
            }
            Event::DeployGossiperAnnouncement(ann) => {
                write!(f, "deploy gossiper announcement: {}", ann)
            }
            Event::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
        }
    }
}

/// The configuration needed to initialize a Validator reactor
#[derive(Debug)]
pub struct ValidatorInitConfig {
    pub(super) root: PathBuf,
    pub(super) config: Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
}

/// Validator node reactor.
#[derive(Debug)]
pub struct Reactor<R: Rng + CryptoRng + ?Sized> {
    metrics: Metrics,
    net: SmallNetwork<Event, Message>,
    address_gossiper: Gossiper<GossipedAddress, Event>,
    storage: Storage,
    contract_runtime: ContractRuntime,
    api_server: ApiServer,
    consensus: EraSupervisor<NodeId, R>,
    deploy_acceptor: DeployAcceptor,
    deploy_fetcher: Fetcher<Deploy>,
    deploy_gossiper: Gossiper<Deploy, Event>,
    deploy_buffer: DeployBuffer,
    block_executor: BlockExecutor,
    proto_block_validator: BlockValidator<ProtoBlock, NodeId>,
    linear_chain: LinearChain<NodeId>,
}

#[cfg(test)]
impl<R: Rng + CryptoRng + ?Sized> Reactor<R> {
    /// Inspect consensus.
    pub(crate) fn consensus(&self) -> &EraSupervisor<NodeId, R> {
        &self.consensus
    }
}

impl<R: Rng + CryptoRng + ?Sized> reactor::Reactor<R> for Reactor<R> {
    type Event = Event;

    // The "configuration" is in fact the whole state of the joiner reactor, which we
    // deconstruct and reuse.
    type Config = ValidatorInitConfig;
    type Error = Error;

    fn new(
        config: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut R,
    ) -> Result<(Self, Effects<Event>), Error> {
        let ValidatorInitConfig {
            root,
            config,
            chainspec_loader,
            storage,
            contract_runtime,
        } = config;

        let metrics = Metrics::new(registry.clone());

        let effect_builder = EffectBuilder::new(event_queue);
        let (net, net_effects) = SmallNetwork::new(event_queue, config.network)?;

        let address_gossiper = Gossiper::new_for_complete_items(config.gossip);

        let api_server = ApiServer::new(config.http_server, effect_builder);
        let timestamp = Timestamp::now();
        let validator_stakes = chainspec_loader
            .chainspec()
            .genesis
            .accounts
            .iter()
            .filter_map(|genesis_account| {
                if genesis_account.is_genesis_validator() {
                    let public_key = genesis_account
                        .public_key()
                        .expect("should have genesis public key");

                    let crypto_public_key = public_key
                        .try_into()
                        .expect("should have valid genesis public key");

                    Some((crypto_public_key, genesis_account.bonded_amount()))
                } else {
                    None
                }
            })
            .collect();
        let (consensus, consensus_effects) = EraSupervisor::new(
            timestamp,
            WithDir::new(root, config.consensus),
            effect_builder,
            validator_stakes,
            &chainspec_loader.chainspec().genesis.highway_config,
            rng,
        )?;
        let deploy_acceptor = DeployAcceptor::new();
        let deploy_fetcher = Fetcher::new(config.gossip);
        let deploy_gossiper = Gossiper::new_for_partial_items(
            config.gossip,
            gossiper::get_deploy_from_storage::<Deploy, Event>,
        );
        let deploy_buffer = DeployBuffer::new(config.node.block_max_deploy_count as usize);
        // Post state hash is expected to be present.
        let genesis_post_state_hash = chainspec_loader
            .genesis_post_state_hash()
            .expect("should have post state hash");
        let block_executor = BlockExecutor::new(genesis_post_state_hash);
        let proto_block_validator = BlockValidator::new();
        let linear_chain = LinearChain::new();

        let mut effects = reactor::wrap_effects(Event::Network, net_effects);
        effects.extend(reactor::wrap_effects(Event::Consensus, consensus_effects));

        Ok((
            Reactor {
                metrics,
                net,
                address_gossiper,
                storage,
                contract_runtime,
                api_server,
                consensus,
                deploy_acceptor,
                deploy_fetcher,
                deploy_gossiper,
                deploy_buffer,
                block_executor,
                proto_block_validator,
                linear_chain,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut R,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.net.handle_event(effect_builder, rng, event),
            ),
            Event::DeployBuffer(event) => reactor::wrap_effects(
                Event::DeployBuffer,
                self.deploy_buffer.handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::ApiServer(event) => reactor::wrap_effects(
                Event::ApiServer,
                self.api_server.handle_event(effect_builder, rng, event),
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
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
            Event::BlockExecutor(event) => reactor::wrap_effects(
                Event::BlockExecutor,
                self.block_executor.handle_event(effect_builder, rng, event),
            ),
            Event::ProtoBlockValidator(event) => reactor::wrap_effects(
                Event::ProtoBlockValidator,
                self.proto_block_validator
                    .handle_event(effect_builder, rng, event),
            ),
            Event::LinearChain(event) => reactor::wrap_effects(
                Event::LinearChain,
                self.linear_chain.handle_event(effect_builder, rng, event),
            ),

            // Requests:
            Event::NetworkRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::Network(small_network::Event::from(req)),
            ),
            Event::DeployFetcherRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::DeployFetcher(req.into()))
            }
            Event::DeployBufferRequest(req) => {
                self.dispatch_event(effect_builder, rng, Event::DeployBuffer(req.into()))
            }
            Event::BlockExecutorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::BlockExecutor(block_executor::Event::from(req)),
            ),
            Event::ProtoBlockValidatorRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::ProtoBlockValidator(block_validator::Event::from(req)),
            ),
            Event::MetricsRequest(req) => reactor::wrap_effects(
                Event::MetricsRequest,
                self.metrics.handle_event(effect_builder, rng, req),
            ),

            // Announcements:
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
                    Message::GetRequest { tag, serialized_id } => match tag {
                        Tag::Deploy => {
                            let deploy_hash = match rmp_serde::from_read_ref(&serialized_id) {
                                Ok(hash) => hash,
                                Err(error) => {
                                    error!(
                                        "failed to decode {:?} from {}: {}",
                                        serialized_id, sender, error
                                    );
                                    return Effects::new();
                                }
                            };
                            Event::Storage(storage::Event::GetDeployForPeer {
                                deploy_hash,
                                peer: sender,
                            })
                        }
                        Tag::Block => {
                            let block_hash = match rmp_serde::from_read_ref(&serialized_id) {
                                Ok(hash) => hash,
                                Err(error) => {
                                    error!(
                                        "failed to decode {:?} from {}: {}",
                                        serialized_id, sender, error
                                    );
                                    return Effects::new();
                                }
                            };
                            Event::LinearChain(linear_chain::Event::Request(
                                LinearChainRequest::BlockRequest(block_hash, sender),
                            ))
                        }
                        Tag::GossipedAddress => {
                            warn!("received get request for gossiped-address from {}", sender);
                            return Effects::new();
                        }
                    },
                    Message::GetResponse {
                        tag,
                        serialized_item,
                    } => match tag {
                        Tag::Deploy => {
                            let deploy = match rmp_serde::from_read_ref(&serialized_item) {
                                Ok(deploy) => Box::new(deploy),
                                Err(error) => {
                                    error!("failed to decode deploy from {}: {}", sender, error);
                                    return Effects::new();
                                }
                            };
                            Event::DeployAcceptor(deploy_acceptor::Event::Accept {
                                deploy,
                                source: Source::Peer(sender),
                            })
                        }
                        Tag::Block => todo!("Handle GET block response"),
                        Tag::GossipedAddress => {
                            warn!("received get request for gossiped-address from {}", sender);
                            return Effects::new();
                        }
                    },
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(gossiped_address)) => {
                let event = gossiper::Event::ItemReceived {
                    item_id: gossiped_address,
                    source: Source::<NodeId>::Client,
                };
                self.dispatch_event(effect_builder, rng, Event::AddressGossiper(event))
            }
            Event::ApiServerAnnouncement(ApiServerAnnouncement::DeployReceived { deploy }) => {
                let event = deploy_acceptor::Event::Accept {
                    deploy,
                    source: Source::<NodeId>::Client,
                };
                self.dispatch_event(effect_builder, rng, Event::DeployAcceptor(event))
            }
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

                let event = gossiper::Event::ItemReceived {
                    item_id: *deploy.id(),
                    source,
                };
                effects.extend(self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::DeployGossiper(event),
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
            Event::ConsensusAnnouncement(consensus_announcement) => {
                let reactor_event = Event::DeployBuffer(match consensus_announcement {
                    ConsensusAnnouncement::Proposed(block) => {
                        deploy_buffer::Event::ProposedProtoBlock(block)
                    }
                    ConsensusAnnouncement::Finalized(block) => {
                        deploy_buffer::Event::FinalizedProtoBlock(block)
                    }
                    ConsensusAnnouncement::Orphaned(block) => {
                        deploy_buffer::Event::OrphanedProtoBlock(block)
                    }
                });
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::BlockExecutorAnnouncement(BlockExecutorAnnouncement::LinearChainBlock(
                block,
            )) => {
                let reactor_event =
                    Event::LinearChain(linear_chain::Event::LinearChainBlock(block));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::DeployGossiperAnnouncement(_ann) => {
                unreachable!("the deploy gossiper should never make an announcement")
            }
            Event::AddressGossiperAnnouncement(ann) => {
                let GossiperAnnouncement::NewCompleteItem(gossiped_address) = ann;
                let reactor_event =
                    Event::Network(small_network::Event::PeerAddressReceived(gossiped_address));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
        }
    }
}

#[cfg(test)]
impl<R: Rng + CryptoRng + ?Sized> NetworkedReactor for Reactor<R> {
    type NodeId = NodeId;
    fn node_id(&self) -> Self::NodeId {
        self.net.node_id()
    }
}
