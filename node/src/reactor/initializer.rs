//! Reactor used to initialize a node.

use std::fmt::{self, Display, Formatter};

use derive_more::From;
use prometheus::Registry;
use rand::Rng;
use thiserror::Error;

use crate::{
    components::{
        chainspec_loader::{self, ChainspecLoader},
        contract_runtime::{self, ContractRuntime},
        small_network::NodeId,
        storage::{self, Storage, StorageType},
        Component,
    },
    effect::{
        requests::{ContractRuntimeRequest, NetworkRequest, StorageRequest},
        EffectBuilder, Effects,
    },
    reactor::{self, validator, EventQueueHandle},
};

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
pub enum Event {
    /// Chainspec handler event.
    #[from]
    Chainspec(chainspec_loader::Event),

    /// Storage event.
    #[from]
    Storage(storage::Event<Storage>),

    /// Contract runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),
}

impl From<StorageRequest<Storage>> for Event {
    fn from(request: StorageRequest<Storage>) -> Self {
        Event::Storage(storage::Event::Request(request))
    }
}

impl From<ContractRuntimeRequest> for Event {
    fn from(request: ContractRuntimeRequest) -> Self {
        Event::ContractRuntime(contract_runtime::Event::Request(request))
    }
}

impl From<NetworkRequest<NodeId, validator::Message>> for Event {
    fn from(_request: NetworkRequest<NodeId, validator::Message>) -> Self {
        unreachable!("no network traffic happens during initialization")
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Chainspec(event) => write!(formatter, "chainspec: {}", event),
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::ContractRuntime(event) => write!(formatter, "contract runtime: {}", event),
        }
    }
}

/// Error type returned by the initializer reactor.
#[derive(Debug, Error)]
pub enum Error {
    /// `Config` error.
    #[error("config error: {0}")]
    ConfigError(String),

    /// Metrics-related error
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),

    /// `ChainspecHandler` component error.
    #[error("chainspec error: {0}")]
    Chainspec(#[from] chainspec_loader::Error),

    /// `Storage` component error.
    #[error("storage error: {0}")]
    Storage(#[from] storage::Error),

    /// `ContractRuntime` component error.
    #[error("contract runtime config error: {0}")]
    ContractRuntime(#[from] contract_runtime::ConfigError),
}

/// Initializer node reactor.
#[derive(Debug)]
pub struct Reactor {
    pub(super) config: validator::Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
}

impl Reactor {
    /// Returns whether the initialization process completed successfully or not.
    pub fn stopped_successfully(&self) -> bool {
        self.chainspec_loader.stopped_successfully()
    }
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = validator::Config;
    type Error = Error;

    fn new<Rd: Rng + ?Sized>(
        config: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut Rd,
    ) -> Result<(Self, Effects<Self::Event>), Error> {
        let chainspec = config
            .node
            .chainspec
            .clone()
            .load()
            .map_err(|err| Error::ConfigError(err.to_string()))?;

        let effect_builder = EffectBuilder::new(event_queue);

        let storage = Storage::new(&config.storage)?;
        let contract_runtime =
            ContractRuntime::new(&config.storage, config.contract_runtime, registry)?;
        let (chainspec_loader, chainspec_effects) =
            ChainspecLoader::new(chainspec, effect_builder)?;

        let effects = reactor::wrap_effects(Event::Chainspec, chainspec_effects);

        Ok((
            Reactor {
                config,
                chainspec_loader,
                storage,
                contract_runtime,
            },
            effects,
        ))
    }

    fn dispatch_event<Rd: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut Rd,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Chainspec(event) => reactor::wrap_effects(
                Event::Chainspec,
                self.chainspec_loader
                    .handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
        }
    }

    fn is_stopped(&mut self) -> bool {
        self.chainspec_loader.is_stopped()
    }
}
