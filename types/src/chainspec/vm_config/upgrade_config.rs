use num_rational::Ratio;
use std::collections::BTreeMap;

use crate::{ChainspecRegistry, Digest, EraId, Key, ProtocolVersion, StoredValue};

/// Represents the configuration of a protocol upgrade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeConfig {
    pre_state_hash: Digest,
    current_protocol_version: ProtocolVersion,
    new_protocol_version: ProtocolVersion,
    activation_point: Option<EraId>,
    new_validator_slots: Option<u32>,
    new_auction_delay: Option<u64>,
    new_locked_funds_period_millis: Option<u64>,
    new_round_seigniorage_rate: Option<Ratio<u64>>,
    new_unbonding_delay: Option<u64>,
    global_state_update: BTreeMap<Key, StoredValue>,
    chainspec_registry: ChainspecRegistry,
}

impl UpgradeConfig {
    /// Create new upgrade config.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pre_state_hash: Digest,
        current_protocol_version: ProtocolVersion,
        new_protocol_version: ProtocolVersion,
        activation_point: Option<EraId>,
        new_validator_slots: Option<u32>,
        new_auction_delay: Option<u64>,
        new_locked_funds_period_millis: Option<u64>,
        new_round_seigniorage_rate: Option<Ratio<u64>>,
        new_unbonding_delay: Option<u64>,
        global_state_update: BTreeMap<Key, StoredValue>,
        chainspec_registry: ChainspecRegistry,
    ) -> Self {
        UpgradeConfig {
            pre_state_hash,
            current_protocol_version,
            new_protocol_version,
            activation_point,
            new_validator_slots,
            new_auction_delay,
            new_locked_funds_period_millis,
            new_round_seigniorage_rate,
            new_unbonding_delay,
            global_state_update,
            chainspec_registry,
        }
    }

    /// Returns the current state root state hash
    pub fn pre_state_hash(&self) -> Digest {
        self.pre_state_hash
    }

    /// Returns current protocol version of this upgrade.
    pub fn current_protocol_version(&self) -> ProtocolVersion {
        self.current_protocol_version
    }

    /// Returns new protocol version of this upgrade.
    pub fn new_protocol_version(&self) -> ProtocolVersion {
        self.new_protocol_version
    }

    /// Returns activation point in eras.
    pub fn activation_point(&self) -> Option<EraId> {
        self.activation_point
    }

    /// Returns new validator slots if specified.
    pub fn new_validator_slots(&self) -> Option<u32> {
        self.new_validator_slots
    }

    /// Returns new auction delay if specified.
    pub fn new_auction_delay(&self) -> Option<u64> {
        self.new_auction_delay
    }

    /// Returns new locked funds period if specified.
    pub fn new_locked_funds_period_millis(&self) -> Option<u64> {
        self.new_locked_funds_period_millis
    }

    /// Returns new round seigniorage rate if specified.
    pub fn new_round_seigniorage_rate(&self) -> Option<Ratio<u64>> {
        self.new_round_seigniorage_rate
    }

    /// Returns new unbonding delay if specified.
    pub fn new_unbonding_delay(&self) -> Option<u64> {
        self.new_unbonding_delay
    }

    /// Returns new map of emergency global state updates.
    pub fn global_state_update(&self) -> &BTreeMap<Key, StoredValue> {
        &self.global_state_update
    }

    /// Returns a reference to the chainspec registry.
    pub fn chainspec_registry(&self) -> &ChainspecRegistry {
        &self.chainspec_registry
    }

    /// Sets new pre state hash.
    pub fn with_pre_state_hash(&mut self, pre_state_hash: Digest) {
        self.pre_state_hash = pre_state_hash;
    }
}
