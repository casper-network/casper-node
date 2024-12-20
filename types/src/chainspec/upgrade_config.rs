use num_rational::Ratio;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::{
    ChainspecRegistry, Digest, EraId, FeeHandling, HoldBalanceHandling, Key, ProtocolVersion,
    StoredValue,
};

/// Represents the configuration of a protocol upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProtocolUpgradeConfig {
    pre_state_hash: Digest,
    current_protocol_version: ProtocolVersion,
    new_protocol_version: ProtocolVersion,
    activation_point: Option<EraId>,
    new_gas_hold_handling: Option<HoldBalanceHandling>,
    new_gas_hold_interval: Option<u64>,
    new_validator_slots: Option<u32>,
    new_auction_delay: Option<u64>,
    new_locked_funds_period_millis: Option<u64>,
    new_round_seigniorage_rate: Option<Ratio<u64>>,
    new_unbonding_delay: Option<u64>,
    global_state_update: BTreeMap<Key, StoredValue>,
    chainspec_registry: ChainspecRegistry,
    fee_handling: FeeHandling,
    maximum_delegation_amount: u64,
    minimum_delegation_amount: u64,
    enable_addressable_entity: bool,
}

impl ProtocolUpgradeConfig {
    /// Create new upgrade config.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pre_state_hash: Digest,
        current_protocol_version: ProtocolVersion,
        new_protocol_version: ProtocolVersion,
        activation_point: Option<EraId>,
        new_gas_hold_handling: Option<HoldBalanceHandling>,
        new_gas_hold_interval: Option<u64>,
        new_validator_slots: Option<u32>,
        new_auction_delay: Option<u64>,
        new_locked_funds_period_millis: Option<u64>,
        new_round_seigniorage_rate: Option<Ratio<u64>>,
        new_unbonding_delay: Option<u64>,
        global_state_update: BTreeMap<Key, StoredValue>,
        chainspec_registry: ChainspecRegistry,
        fee_handling: FeeHandling,
        maximum_delegation_amount: u64,
        minimum_delegation_amount: u64,
        enable_addressable_entity: bool,
    ) -> Self {
        ProtocolUpgradeConfig {
            pre_state_hash,
            current_protocol_version,
            new_protocol_version,
            activation_point,
            new_gas_hold_handling,
            new_gas_hold_interval,
            new_validator_slots,
            new_auction_delay,
            new_locked_funds_period_millis,
            new_round_seigniorage_rate,
            new_unbonding_delay,
            global_state_update,
            chainspec_registry,
            fee_handling,
            maximum_delegation_amount,
            minimum_delegation_amount,
            enable_addressable_entity,
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

    /// Returns new gas hold handling if specified.
    pub fn new_gas_hold_handling(&self) -> Option<HoldBalanceHandling> {
        self.new_gas_hold_handling
    }

    /// Returns new auction delay if specified.
    pub fn new_gas_hold_interval(&self) -> Option<u64> {
        self.new_gas_hold_interval
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

    /// Fee handling setting.
    pub fn fee_handling(&self) -> FeeHandling {
        self.fee_handling
    }

    /// Maximum delegation amount for validator.
    pub fn maximum_delegation_amount(&self) -> u64 {
        self.maximum_delegation_amount
    }

    /// Minimum delegation amount for validator.
    pub fn minimum_delegation_amount(&self) -> u64 {
        self.minimum_delegation_amount
    }

    pub fn enable_addressable_entity(&self) -> bool {
        self.enable_addressable_entity
    }
}
