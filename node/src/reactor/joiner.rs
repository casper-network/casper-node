//! Reactor used to join the network.

use std::{
    fmt::{self, Display, Formatter},
    path::PathBuf,
};

use derive_more::From;
use prometheus::Registry;
use rand::{CryptoRng, Rng};

use crate::{
    components::{
        chainspec_loader::ChainspecLoader,
        contract_runtime::ContractRuntime,
        fetcher::{self, Fetcher},
        linear_chain_sync::{self, LinearChainSync},
        small_network,
        small_network::{NodeId, SmallNetwork},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::NetworkAnnouncement,
        requests::{FetcherRequest, NetworkRequest, StorageRequest},
        EffectBuilder, Effects,
    },
    protocol::Message,
    reactor::{
        self, initializer,
        validator::{self, Error, ValidatorInitConfig},
        EventQueueHandle, Finalize,
    },
    types::Block,
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

    /// Linear chain event.
    #[from]
    LinearChainSync(linear_chain_sync::Event<NodeId>),

    /// Requests.
    /// Linear chain fetcher request.
    #[from]
    BlockFetcherRequest(FetcherRequest<NodeId, Block>),
    // Announcements
    /// Network announcement.
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
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

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(f, "network: {}", event),
            Event::NetworkAnnouncement(event) => write!(f, "network announcement: {}", event),
            Event::Storage(request) => write!(f, "storage: {}", request),
            Event::BlockFetcherRequest(request) => write!(f, "block fetcher request: {}", request),
            Event::LinearChainSync(event) => write!(f, "linear chain: {}", event),
            Event::BlockFetcher(event) => write!(f, "block fetcher: {}", event),
        }
    }
}

/// Joining node reactor.
#[derive(Debug)]
pub struct Reactor {
    pub(super) root: PathBuf,
    pub(super) net: SmallNetwork<Event, Message>,
    pub(super) config: validator::Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
    pub(super) linear_chain_fetcher: Fetcher<Block>,
    pub(super) linear_chain_sync: LinearChainSync<NodeId>,
}

impl<R: Rng + CryptoRng + ?Sized> reactor::Reactor<R> for Reactor {
    type Event = Event;

    // The "configuration" is in fact the whole state of the initializer reactor, which we
    // deconstruct and reuse.
    type Config = WithDir<initializer::Reactor>;
    type Error = Error;

    fn new(
        initializer: Self::Config,
        _registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut R,
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

        Ok((
            Self {
                net,
                root,
                config,
                chainspec_loader,
                storage,
                contract_runtime,
                linear_chain_sync,
                linear_chain_fetcher,
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
            Event::NetworkAnnouncement(NetworkAnnouncement::NewPeer(id)) => reactor::wrap_effects(
                Event::LinearChainSync,
                self.linear_chain_sync.handle_event(
                    effect_builder,
                    rng,
                    linear_chain_sync::Event::NewPeerConnected(id),
                ),
            ),
            Event::NetworkAnnouncement(_) => Default::default(),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::BlockFetcherRequest(request) => {
                self.dispatch_event(effect_builder, rng, Event::BlockFetcher(request.into()))
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
        }
    }

    fn is_stopped(&mut self) -> bool {
        self.linear_chain_sync.is_synced()
    }
}

impl Reactor {
    /// Deconstructs the reactor into config useful for creating a Validator reactor. Shuts down
    /// the network, closing all incoming and outgoing connections, and frees up the listening
    /// socket.
    pub async fn into_validator_config(self) -> ValidatorInitConfig {
        let (net, config) = (
            self.net,
            ValidatorInitConfig {
                root: self.root,
                chainspec_loader: self.chainspec_loader,
                config: self.config,
                contract_runtime: self.contract_runtime,
                storage: self.storage,
            },
        );
        net.finalize().await;
        config
    }
}
