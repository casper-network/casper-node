//! Reactor used to initialize a node.

use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use thiserror::Error;
use tracing::info;

use casper_execution_engine::core::engine_state;

use crate::{
    components::{
        chainspec_loader::{self, ChainspecLoader},
        contract_runtime::{self, ContractRuntime},
        gossiper,
        small_network::{GossipedAddress, SmallNetworkIdentity, SmallNetworkIdentityError},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{
            ChainspecLoaderAnnouncement, ContractRuntimeAnnouncement, ControlAnnouncement,
        },
        requests::{
            ConsensusRequest, ContractRuntimeRequest, NetworkRequest, RestRequest, StorageRequest,
        },
        EffectBuilder, Effects,
    },
    protocol::Message,
    reactor::{self, participating, EventQueueHandle, ReactorExit},
    types::{chainspec, NodeId},
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
    ContractRuntime(#[serde(skip_serializing)] Box<ContractRuntimeRequest>),

    /// Control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),

    /// Storage request.
    #[from]
    StorageRequest(StorageRequest),
}

impl From<ContractRuntimeRequest> for Event {
    fn from(contract_runtime_request: ContractRuntimeRequest) -> Self {
        Event::ContractRuntime(Box::new(contract_runtime_request))
    }
}

impl ReactorEvent for Event {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Event::Chainspec(_) => "Chainspec",
            Event::Storage(_) => "Storage",
            Event::ContractRuntime(_) => "ContractRuntime",
            Event::ControlAnnouncement(_) => "ControlAnnouncement",
            Event::StorageRequest(_) => "StorageRequest",
        }
    }
}

impl From<NetworkRequest<NodeId, Message>> for Event {
    fn from(_request: NetworkRequest<NodeId, Message>) -> Self {
        unreachable!("no network traffic happens during initialization")
    }
}

impl From<ChainspecLoaderAnnouncement> for Event {
    fn from(_announcement: ChainspecLoaderAnnouncement) -> Self {
        unreachable!("no chainspec announcements happen during initialization")
    }
}

impl From<NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>> for Event {
    fn from(_request: NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>) -> Self {
        unreachable!("no gossiper events happen during initialization")
    }
}

impl From<ConsensusRequest> for Event {
    fn from(_request: ConsensusRequest) -> Self {
        unreachable!("no chainspec announcements happen during initialization")
    }
}

impl From<RestRequest<NodeId>> for Event {
    fn from(_request: RestRequest<NodeId>) -> Self {
        unreachable!("no rest requests happen during initialization")
    }
}

impl From<ContractRuntimeAnnouncement> for Event {
    fn from(_request: ContractRuntimeAnnouncement) -> Self {
        unreachable!("no block executor requests happen during initialization")
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Chainspec(event) => write!(formatter, "chainspec: {}", event),
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::ContractRuntime(event) => write!(formatter, "contract runtime: {:?}", event),
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::StorageRequest(req) => write!(formatter, "storage request: {}", req),
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

    /// Trie key store is corrupted (missing trie keys).
    #[error(
        "Missing trie keys. Number of state roots: {state_root_count}, \
         Number of missing trie keys: {missing_trie_key_count}"
    )]
    MissingTrieKeys {
        /// The number of state roots in all of the block headers.
        state_root_count: usize,
        /// The number of trie keys we could not find.
        missing_trie_key_count: usize,
    },
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
        (should_check_integrity, config): <Self as reactor::Reactor>::Config,
        registry: &Registry,
        chainspec_loader: ChainspecLoader,
        chainspec_effects: Effects<chainspec_loader::Event>,
    ) -> Result<(Self, Effects<Event>), Error> {
        let hard_reset_to_start_of_era = chainspec_loader.hard_reset_to_start_of_era();

        let storage_config = config.map_ref(|cfg| cfg.storage.clone());
        let storage = Storage::new(
            &storage_config,
            hard_reset_to_start_of_era,
            should_check_integrity,
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
            registry,
            chainspec_loader
                .chainspec()
                .protocol_config
                .verifiable_chunked_hash_activation,
        )?;

        if should_check_integrity {
            info!("running trie-store integrity check, this may take a while");
            let state_roots = storage.read_state_root_hashes_for_trie_check()?;
            let missing_trie_keys = contract_runtime.trie_store_check(state_roots.clone())?;
            if !missing_trie_keys.is_empty() {
                return Err(Error::MissingTrieKeys {
                    state_root_count: state_roots.len(),
                    missing_trie_key_count: missing_trie_keys.len(),
                });
            }
        }

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
    type Config = (bool, WithDir<participating::Config>);
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
            ChainspecLoader::new(config.1.dir(), effect_builder)?;
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
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::ContractRuntime(event) => reactor::wrap_effects(
                Event::from,
                self.contract_runtime
                    .handle_event(effect_builder, rng, *event),
            ),
            Event::StorageRequest(req) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            Event::ControlAnnouncement(_) => unreachable!("unhandled control announcement"),
        }
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        self.chainspec_loader.reactor_exit()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{testing::network::NetworkedReactor, types::Chainspec};
    use std::sync::Arc;

    impl Reactor {
        pub(crate) fn new_with_chainspec(
            config: <Self as reactor::Reactor>::Config,
            registry: &Registry,
            event_queue: EventQueueHandle<Event>,
            chainspec: Arc<Chainspec>,
        ) -> Result<(Self, Effects<Event>), Error> {
            let effect_builder = EffectBuilder::new(event_queue);
            let (chainspec_loader, chainspec_effects) =
                ChainspecLoader::new_with_chainspec(chainspec, effect_builder);
            Self::new_with_chainspec_loader(config, registry, chainspec_loader, chainspec_effects)
        }
    }

    impl NetworkedReactor for Reactor {
        type NodeId = NodeId;
        fn node_id(&self) -> Self::NodeId {
            NodeId::from(&self.small_network_identity)
        }
    }
}
