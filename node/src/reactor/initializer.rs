//! Reactor used to initialize a node.

use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use derive_more::From;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use thiserror::Error;
use tracing::{error, info, warn};

use casper_execution_engine::core::engine_state;
use casper_types::{
    bytesrepr::{U64_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH},
    KEY_HASH_LENGTH,
};

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
    use std::{collections::BTreeMap, sync::Arc};

    use super::*;

    use casper_execution_engine::core::engine_state::engine_config::DEFAULT_MAX_STORED_VALUE_SIZE;
    use casper_types::{
        bytesrepr::ToBytes,
        system::auction::{
            DelegationRate, SeigniorageRecipient, SeigniorageRecipients,
            SeigniorageRecipientsSnapshot,
        },
        EraId, PublicKey, SecretKey, U256, U512,
    };

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
