//! Reactor used to initialize a node.

use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use thiserror::Error;
use tracing::{error, warn};

use casper_execution_engine::core::engine_state;

use crate::{
    components::{
        chainspec_loader::{self, ChainspecLoader},
        contract_runtime::{self, ContractRuntime},
        small_network::{SmallNetworkIdentity, SmallNetworkIdentityError},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            ChainspecLoaderAnnouncement, ContractRuntimeAnnouncement, ControlAnnouncement,
        },
        requests::{
            ChainspecLoaderRequest, ContractRuntimeRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, Effects,
    },
    protocol::Message,
    reactor::{self, participating, EventQueueHandle, ReactorExit},
    types::chainspec,
    utils::WithDir,
    NodeRng,
};

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
pub(crate) enum Event {
    /// Chainspec handler event.
    #[from]
    Chainspec(chainspec_loader::Event),

    /// Storage event.
    #[from]
    Storage(storage::Event),

    /// Contract runtime event.
    #[from]
    ContractRuntime(contract_runtime::Event),

    /// Control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),

    /// Chainspec loader announcement.
    #[from]
    ChainspecLoaderAnnouncement(#[serde(skip_serializing)] ChainspecLoaderAnnouncement),

    /// Contract runtime announcement.
    #[from]
    ContractRuntimeAnnouncement(#[serde(skip_serializing)] ContractRuntimeAnnouncement),

    /// ChainspecLoader request.
    #[from]
    ChainspecLoaderRequest(ChainspecLoaderRequest),

    /// Storage request.
    #[from]
    StorageRequest(StorageRequest),

    /// Contract runtime request.
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),

    // Network request.
    #[from]
    NetworkRequest(NetworkRequest<Message>),
}

impl ReactorEvent for Event {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Event::Chainspec(_) => "Chainspec",
            Event::Storage(_) => "Storage",
            Event::ContractRuntimeRequest(_) => "ContractRuntimeRequest",
            Event::ControlAnnouncement(_) => "ControlAnnouncement",
            Event::StorageRequest(_) => "StorageRequest",
            Event::ContractRuntime(_) => "ContractRuntime",
            Event::ChainspecLoaderAnnouncement(_) => "ChainspecLoaderAnnouncement",
            Event::ContractRuntimeAnnouncement(_) => "ContractRuntimeAnnouncement",
            Event::NetworkRequest(_) => "NetworkRequest",
            Event::ChainspecLoaderRequest(_) => "ChainspecLoaderRequest",
        }
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Chainspec(event) => write!(formatter, "chainspec: {}", event),
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::ContractRuntimeRequest(event) => {
                write!(formatter, "contract runtime request: {:?}", event)
            }
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::StorageRequest(req) => write!(formatter, "storage request: {}", req),
            Event::ContractRuntime(event) => write!(formatter, "contract runtime event: {}", event),
            Event::ChainspecLoaderAnnouncement(ann) => {
                write!(formatter, "chainspec loader announcement: {}", ann)
            }
            Event::ContractRuntimeAnnouncement(ann) => {
                write!(formatter, "contract runtime announcement: {}", ann)
            }
            Event::NetworkRequest(request) => write!(formatter, "network request: {:?}", request),
            Event::ChainspecLoaderRequest(req) => {
                write!(formatter, "chainspec_loader request: {}", req)
            }
        }
    }
}

/// Error type returned by the initializer reactor.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Metrics-related error
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),

    /// `ChainspecHandler` component error.
    #[error("chainspec error: {0}")]
    Chainspec(#[from] chainspec::Error),

    /// `Storage` component error.
    #[error("storage error: {0}")]
    Storage(#[from] storage::FatalStorageError),

    /// `ContractRuntime` component error.
    #[error("contract runtime config error: {0}")]
    ContractRuntime(#[from] contract_runtime::ConfigError),

    /// An error that occurred when creating a `SmallNetworkIdentity`.
    #[error(transparent)]
    SmallNetworkIdentity(#[from] SmallNetworkIdentityError),

    /// An execution engine state error.
    #[error(transparent)]
    EngineState(#[from] engine_state::Error),
}

/// Initializer node reactor.
#[derive(DataSize, Debug)]
pub(crate) struct Reactor {
    pub(super) config: WithDir<participating::Config>,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
    pub(super) small_network_identity: SmallNetworkIdentity,
}

impl Reactor {
    fn new_with_chainspec_loader(
        config: <Self as reactor::Reactor>::Config,
        registry: &Registry,
        chainspec_loader: ChainspecLoader,
        chainspec_effects: Effects<chainspec_loader::Event>,
    ) -> Result<(Self, Effects<Event>), Error> {
        let hard_reset_to_start_of_era = chainspec_loader.hard_reset_to_start_of_era();

        let storage_config = config.map_ref(|cfg| cfg.storage.clone());
        let storage = Storage::new(
            &storage_config,
            hard_reset_to_start_of_era,
            chainspec_loader.chainspec().protocol_config.version,
            &chainspec_loader.chainspec().network_config.name,
            chainspec_loader
                .chainspec()
                .highway_config
                .finality_threshold_fraction,
            chainspec_loader
                .chainspec()
                .protocol_config
                .last_emergency_restart,
            chainspec_loader
                .chainspec()
                .protocol_config
                .verifiable_chunked_hash_activation,
        )?;

        let contract_runtime = ContractRuntime::new(
            chainspec_loader.chainspec().protocol_config.version,
            storage.root_path(),
            &config.value().contract_runtime,
            chainspec_loader.chainspec().wasm_config,
            chainspec_loader.chainspec().system_costs_config,
            chainspec_loader.chainspec().core_config.max_associated_keys,
            chainspec_loader
                .chainspec()
                .core_config
                .max_runtime_call_stack_height,
            chainspec_loader
                .chainspec()
                .core_config
                .minimum_delegation_amount,
            chainspec_loader
                .chainspec()
                .core_config
                .strict_argument_checking,
            registry,
            chainspec_loader
                .chainspec()
                .protocol_config
                .verifiable_chunked_hash_activation,
        )?;

        let effects = reactor::wrap_effects(Event::Chainspec, chainspec_effects);

        let small_network_identity = SmallNetworkIdentity::new()?;

        let reactor = Reactor {
            config,
            chainspec_loader,
            storage,
            contract_runtime,
            small_network_identity,
        };
        Ok((reactor, effects))
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

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = WithDir<participating::Config>;
    type Error = Error;

    fn new(
        config: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Error> {
        let effect_builder = EffectBuilder::new(event_queue);

        // Construct the `ChainspecLoader` first so we fail fast if the chainspec is invalid.
        let (chainspec_loader, chainspec_effects) =
            ChainspecLoader::new(config.dir(), effect_builder)?;
        Self::new_with_chainspec_loader(config, registry, chainspec_loader, chainspec_effects)
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Chainspec(event) => reactor::wrap_effects(
                Event::Chainspec,
                self.chainspec_loader
                    .handle_event(effect_builder, rng, event),
            ),
            Event::ChainspecLoaderRequest(event) => reactor::wrap_effects(
                Event::Chainspec,
                self.chainspec_loader.handle_event(
                    effect_builder,
                    rng,
                    chainspec_loader::Event::Request(event),
                ),
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
            Event::ContractRuntimeRequest(event) => reactor::wrap_effects(
                Event::ContractRuntime,
                self.contract_runtime
                    .handle_event(effect_builder, rng, event.into()),
            ),
            Event::StorageRequest(req) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            Event::ControlAnnouncement(ann) => {
                error!(%ann, "control announcement dispatched in initializer");
                Effects::new()
            }
            Event::ChainspecLoaderAnnouncement(ann) => {
                // We don't dispatch ChainspecLoaderAnnouncement as it is ignored in the
                // initializer. This indicates a situation that is not harmful but
                // theoretically shouldn't happen, hence the warning.
                warn!(%ann, "chainspec loader announcement received by initializer, ignoring");
                Effects::new()
            }
            Event::ContractRuntimeAnnouncement(ann) => {
                // We don't dispatch ContractRuntimeAnnouncement as it shouldn't actually arrive at
                // the initializer. This indicates a possible bug.
                error!(%ann, "contract runtime announcement received by initializer, possibly a bug");
                Effects::new()
            }
            Event::NetworkRequest(ann) => {
                // No network traffic is expected during initialization. This indicates a possible
                // bug.
                error!(%ann, "network request received by initializer, possibly a bug");
                Effects::new()
            }
        }
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        self.chainspec_loader.reactor_exit()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use super::*;

    use crate::{
        testing::network::NetworkedReactor,
        types::{Chainspec, ChainspecRawBytes, NodeId},
    };

    impl Reactor {
        pub(crate) fn new_with_chainspec(
            config: <Self as reactor::Reactor>::Config,
            registry: &Registry,
            event_queue: EventQueueHandle<Event>,
            chainspec: Arc<Chainspec>,
            chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        ) -> Result<(Self, Effects<Event>), Error> {
            let effect_builder = EffectBuilder::new(event_queue);
            let (chainspec_loader, chainspec_effects) =
                ChainspecLoader::new_with_chainspec(chainspec, chainspec_raw_bytes, effect_builder);
            Self::new_with_chainspec_loader(config, registry, chainspec_loader, chainspec_effects)
        }
    }

    impl NetworkedReactor for Reactor {
        fn node_id(&self) -> NodeId {
            NodeId::from(&self.small_network_identity)
        }
    }
}
