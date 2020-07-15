//! Reactor used to initialize a node.

use std::{
    fmt::{self, Display, Formatter},
    path::PathBuf,
};

use derive_more::From;
use rand::Rng;
use thiserror::Error;
use tracing::debug;

use crate::{
    components::{
        chainspec_handler::{self, ChainspecHandler},
        contract_runtime::{self, ContractRuntime},
        storage::{self, Storage, StorageType},
        Component,
    },
    effect::{
        announcements::StorageAnnouncement,
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder, Effects,
    },
    reactor::{self, validator, EventQueueHandle},
    types::DeployHash,
};

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
pub enum Event {
    /// Chainspec handler event.
    #[from]
    Chainspec(chainspec_handler::Event),

    /// Storage event.
    #[from]
    Storage(StorageRequest<Storage>),

    /// Storage announcement.
    #[from]
    StorageAnnouncement(StorageAnnouncement<DeployHash>),

    /// Contract runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),
}

impl From<ContractRuntimeRequest> for Event {
    fn from(request: ContractRuntimeRequest) -> Self {
        Event::ContractRuntime(contract_runtime::Event::Request(request))
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Chainspec(event) => write!(formatter, "chainspec: {}", event),
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::StorageAnnouncement(announcement) => {
                write!(formatter, "storage announcement: {}", announcement)
            }
            Event::ContractRuntime(event) => write!(formatter, "contract runtime: {}", event),
        }
    }
}

/// Error type returned by the initializer reactor.
#[derive(Debug, Error)]
pub enum Error {
    /// `ChainspecHandler` component error.
    #[error("chainspec error: {0}")]
    Chainspec(#[from] chainspec_handler::Error),

    /// `Storage` component error.
    #[error("storage error: {0}")]
    Storage(#[from] storage::Error),

    /// `ContractRuntime` component error.
    #[error("contract runtime config error: {0}")]
    ContractRuntime(#[from] contract_runtime::ConfigError),
}

/// Validator node reactor.
#[derive(Debug)]
pub struct Reactor {
    config: validator::Config,
    chainspec_handler: ChainspecHandler,
    storage: Storage,
    contract_runtime: ContractRuntime,
}

impl Reactor {
    /// Returns whether the initialization process completed successfully or not.
    pub fn stopped_successfully(&self) -> bool {
        self.chainspec_handler.stopped_successfully()
    }

    pub(super) fn destructure(self) -> (validator::Config, Storage, ContractRuntime) {
        (self.config, self.storage, self.contract_runtime)
    }
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = (PathBuf, validator::Config);
    type Error = Error;

    fn new<Rd: Rng + ?Sized>(
        (chainspec_config_path, config): Self::Config,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut Rd,
    ) -> Result<(Self, Effects<Self::Event>), Error> {
        let effect_builder = EffectBuilder::new(event_queue);

        let storage = Storage::new(&config.storage)?;
        let contract_runtime = ContractRuntime::new(&config.storage, config.contract_runtime)?;

        let (chainspec_handler, chainspec_effects) =
            ChainspecHandler::new(chainspec_config_path, effect_builder)?;

        let effects = reactor::wrap_effects(Event::Chainspec, chainspec_effects);

        Ok((
            Reactor {
                config,
                chainspec_handler,
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
                self.chainspec_handler
                    .handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::StorageAnnouncement(announcement) => {
                debug!(%announcement, "ignoring storing announcement");
                Effects::new()
            }
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event),
            ),
        }
    }

    fn is_stopped(&mut self) -> bool {
        self.chainspec_handler.is_stopped()
    }
}
