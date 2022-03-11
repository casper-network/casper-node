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
use casper_types::{
    bytesrepr::{U64_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH},
    KEY_HASH_LENGTH,
};

use crate::{
    components::{
        chainspec_loader::{self, ChainspecLoader},
        contract_runtime::{self, ContractRuntime, ContractRuntimeAnnouncement},
        gossiper,
        small_network::{GossipedAddress, SmallNetworkIdentity, SmallNetworkIdentityError},
        storage::{self, Storage},
        Component,
    },
    effect::{
        announcements::{ChainspecLoaderAnnouncement, ControlAnnouncement},
        requests::{
            ConsensusRequest, ContractRuntimeRequest, LinearChainRequest, NetworkRequest,
            RestRequest, StateStoreRequest, StorageRequest,
        },
        EffectBuilder, Effects,
    },
    protocol::Message,
    reactor::{self, participating, EventQueueHandle, ReactorExit},
    types::{chainspec, NodeId},
    utils::WithDir,
    NodeRng,
};

/// The size in bytes per delegator entry in SeigniorageRecipient struct.
/// 34 bytes PublicKey + 10 bytes U512 for the delegated amount
const SIZE_PER_DELEGATOR_ENTRY: u32 = 44;
/// The fixed portion, in bytes, of the SeigniorageRecipient struct and its key in the
/// `SeigniorageRecipients` map.
/// 34 bytes for the public key + 10 bytes validator weight + 1 byte for delegation rate.
const FIXED_SIZE_PER_VALIDATOR: u32 = 45;
/// The size of a key of the `SeigniorageRecipientsSnapshot`, i.e. an `EraId`.
const FIXED_SIZE_PER_ERA: u32 = U64_SERIALIZED_LENGTH as u32;
/// The overhead of the Key::Hash under which the seigniorage snapshot lives.
/// The hash length plus an additional byte for the tag.
const KEY_HASH_SERIALIZED_LENGTH: u32 = KEY_HASH_LENGTH as u32 + 1;

fn compute_max_delegator_size_limit(
    max_stored_value_size: u32,
    auction_delay: u64,
    validator_slots: u32,
) -> u32 {
    let size_limit_per_snapshot =
        (max_stored_value_size - U8_SERIALIZED_LENGTH as u32 - KEY_HASH_SERIALIZED_LENGTH)
            / (auction_delay + 1) as u32;
    let size_per_seigniorage_recipients = size_limit_per_snapshot - FIXED_SIZE_PER_ERA;
    let size_limit_per_validator =
        (size_per_seigniorage_recipients / validator_slots) - FIXED_SIZE_PER_VALIDATOR;
    // The max number of the delegators per validator is the size limit allotted
    // to a single validator divided by the size of a single delegator entry.
    // For the given:
    // 1. max limit of 8MB
    // 2. 100 validator slots
    // 3. an auction delay of 1
    // There will be a maximum of roughly 953 delegators per validator.
    size_limit_per_validator / SIZE_PER_DELEGATOR_ENTRY
}

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
pub(crate) enum Event {
    /// Chainspec handler event.
    #[from]
    Chainspec(chainspec_loader::Event),

    /// Storage event.

    #[from]
    Storage(#[serde(skip_serializing)] storage::Event),

    /// Contract runtime event.
    ContractRuntime(#[serde(skip_serializing)] Box<ContractRuntimeRequest>),

    /// Request for state storage.
    #[from]
    StateStoreRequest(StateStoreRequest),

    /// Control announcement
    #[from]
    ControlAnnouncement(ControlAnnouncement),
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
            Event::StateStoreRequest(_) => "StateStoreRequest",
            Event::ControlAnnouncement(_) => "ControlAnnouncement",
        }
    }
}

impl From<StorageRequest> for Event {
    fn from(request: StorageRequest) -> Self {
        Event::Storage(storage::Event::StorageRequest(request))
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

impl From<LinearChainRequest<NodeId>> for Event {
    fn from(_req: LinearChainRequest<NodeId>) -> Self {
        unreachable!("no linear chain events happen during initialization")
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
            Event::StateStoreRequest(request) => {
                write!(formatter, "state store request: {}", request)
            }
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
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
    Storage(#[from] storage::Error),

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
        (crashed, config): <Self as reactor::Reactor>::Config,
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
            crashed,
            &chainspec_loader.chainspec().network_config.name,
        )?;

        let max_delegator_size_limit = compute_max_delegator_size_limit(
            chainspec_loader
                .chainspec()
                .core_config
                .max_stored_value_size,
            chainspec_loader.chainspec().core_config.auction_delay,
            chainspec_loader.chainspec().core_config.validator_slots,
        );

        info!(
            "the maximum delegators per validator is currently set to: {}",
            max_delegator_size_limit
        );

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
                .max_stored_value_size,
            max_delegator_size_limit,
            chainspec_loader
                .chainspec()
                .core_config
                .minimum_delegation_amount,
            chainspec_loader
                .chainspec()
                .core_config
                .strict_argument_checking,
            registry,
        )?;

        // TODO: This integrity check is misplaced, it should be part of the components
        // `handle_event` function. Ideally it would be in the constructor, but since a query to
        // storage needs to be made, this is not possible.
        //
        // Refactoring this has been postponed for now, since it is unclear whether time-consuming
        // integrity checks are even a good idea, as they can block the node for one or more hours
        // on restarts (online checks are an alternative).
        if crashed {
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
            Event::StateStoreRequest(request) => {
                self.dispatch_event(effect_builder, rng, Event::Storage(request.into()))
            }
            Event::ControlAnnouncement(_) => unreachable!("unhandled control announcement"),
        }
    }

    fn maybe_exit(&self) -> Option<ReactorExit> {
        self.chainspec_loader.reactor_exit()
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::{testing::network::NetworkedReactor, types::Chainspec};
    use casper_execution_engine::core::engine_state::engine_config::DEFAULT_MAX_STORED_VALUE_SIZE;
    use casper_types::{
        bytesrepr::ToBytes,
        system::auction::{
            DelegationRate, SeigniorageRecipient, SeigniorageRecipients,
            SeigniorageRecipientsSnapshot,
        },
        EraId, PublicKey, SecretKey, U256, U512,
    };
    use std::{collections::BTreeMap, sync::Arc};

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

    fn gen_public_keys() -> impl Iterator<Item = PublicKey> {
        (1u32..)
            .map(U256::from)
            .map(|u256| {
                let mut bytes = [0; 32];
                u256.to_little_endian(&mut bytes);
                SecretKey::secp256k1_from_bytes(&bytes).unwrap()
            })
            .map(|secret_key| PublicKey::from(&secret_key))
    }

    fn generate_seigniorage_snapshot(
        number_of_delegators_per_validator: u32,
        auction_delay: u64,
        validator_slots: u32,
    ) -> SeigniorageRecipientsSnapshot {
        const CSPR: usize = 1_000_000_000;
        const STAKE: usize = 5_000_000_000 * CSPR;
        const DELEGATION_RATE: DelegationRate = 15;

        let stake = U512::from(STAKE) * U512::from(10);
        let mut public_keys = gen_public_keys();

        let mut snapshot = SeigniorageRecipientsSnapshot::new();
        let mut delegator_stake = BTreeMap::new();

        for _ in 0..(number_of_delegators_per_validator) {
            let delegator_public_key = public_keys.next().unwrap();
            delegator_stake.insert(delegator_public_key, stake);
        }

        for era_id in 1..=(auction_delay + 1) {
            let mut seigniorage_recipients = SeigniorageRecipients::new();
            for _ in 0..validator_slots {
                let validator_public_key = public_keys.next().unwrap();
                let seigniorage_recipient =
                    SeigniorageRecipient::new(stake, DELEGATION_RATE, delegator_stake.clone());
                seigniorage_recipients.insert(validator_public_key, seigniorage_recipient);
            }
            snapshot.insert(EraId::new(era_id), seigniorage_recipients);
        }

        snapshot
    }

    #[test]
    fn should_compute_and_match_size_of_seigniorage_snapshot() {
        const AUCTION_DELAY: u64 = 1;
        const VALIDATOR_SLOTS: u32 = 100;

        let number_of_delegators_per_validator = compute_max_delegator_size_limit(
            DEFAULT_MAX_STORED_VALUE_SIZE,
            AUCTION_DELAY,
            VALIDATOR_SLOTS,
        );

        // A snapshot created with a tolerable amount of delegators per validator.
        let correct_snapshot = generate_seigniorage_snapshot(
            number_of_delegators_per_validator,
            AUCTION_DELAY,
            VALIDATOR_SLOTS,
        );

        // Assert that the size of the snapshot with the correct amount of delegators
        // doesn't exceed the maximum limit.
        assert!(correct_snapshot.serialized_length() <= DEFAULT_MAX_STORED_VALUE_SIZE as usize);

        // A snapshot created with an intolerable amount of delegators per validator.
        let incorrect_snapshot = generate_seigniorage_snapshot(
            number_of_delegators_per_validator + 1,
            AUCTION_DELAY,
            VALIDATOR_SLOTS,
        );

        // Assert that the size of the snapshot with the incorrect amount of delegators
        // does exceed the maximum limit.
        assert!(incorrect_snapshot.serialized_length() > DEFAULT_MAX_STORED_VALUE_SIZE as usize);
    }
}
