use std::collections::BTreeMap;

use num_rational::Ratio;

use casper_types::{
    ChainspecRegistry, Digest, EraId, FeeHandling, HoldBalanceHandling, Key, ProtocolUpgradeConfig,
    ProtocolVersion, StoredValue,
};

/// Builds an `UpgradeConfig`.
pub struct UpgradeRequestBuilder {
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

impl UpgradeRequestBuilder {
    /// Returns a new `UpgradeRequestBuilder`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets a pre-state hash using a [`Digest`].
    pub fn with_pre_state_hash(mut self, pre_state_hash: Digest) -> Self {
        self.pre_state_hash = pre_state_hash;
        self
    }

    /// Sets `current_protocol_version` to the given [`ProtocolVersion`].
    pub fn with_current_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.current_protocol_version = protocol_version;
        self
    }

    /// Sets `new_protocol_version` to the given [`ProtocolVersion`].
    pub fn with_new_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.new_protocol_version = protocol_version;
        self
    }

    /// Sets `with_new_gas_hold_handling`.
    pub fn with_new_gas_hold_handling(mut self, gas_hold_handling: HoldBalanceHandling) -> Self {
        self.new_gas_hold_handling = Some(gas_hold_handling);
        self
    }

    /// Sets `with_new_gas_hold_interval`.
    pub fn with_new_gas_hold_interval(mut self, gas_hold_interval: u64) -> Self {
        self.new_gas_hold_interval = Some(gas_hold_interval);
        self
    }

    /// Sets `new_validator_slots`.
    pub fn with_new_validator_slots(mut self, new_validator_slots: u32) -> Self {
        self.new_validator_slots = Some(new_validator_slots);
        self
    }

    /// Sets `new_auction_delay`.
    pub fn with_new_auction_delay(mut self, new_auction_delay: u64) -> Self {
        self.new_auction_delay = Some(new_auction_delay);
        self
    }

    /// Sets `new_locked_funds_period_millis`.
    pub fn with_new_locked_funds_period_millis(
        mut self,
        new_locked_funds_period_millis: u64,
    ) -> Self {
        self.new_locked_funds_period_millis = Some(new_locked_funds_period_millis);
        self
    }

    /// Sets `new_round_seigniorage_rate`.
    pub fn with_new_round_seigniorage_rate(mut self, rate: Ratio<u64>) -> Self {
        self.new_round_seigniorage_rate = Some(rate);
        self
    }

    /// Sets `new_unbonding_delay`.
    pub fn with_new_unbonding_delay(mut self, unbonding_delay: u64) -> Self {
        self.new_unbonding_delay = Some(unbonding_delay);
        self
    }

    /// Sets `global_state_update`.
    pub fn with_global_state_update(
        mut self,
        global_state_update: BTreeMap<Key, StoredValue>,
    ) -> Self {
        self.global_state_update = global_state_update;
        self
    }

    /// Sets `activation_point`.
    pub fn with_activation_point(mut self, activation_point: EraId) -> Self {
        self.activation_point = Some(activation_point);
        self
    }

    /// Sets the Chainspec registry.
    pub fn with_chainspec_registry(mut self, chainspec_registry: ChainspecRegistry) -> Self {
        self.chainspec_registry = chainspec_registry;
        self
    }

    /// Sets the fee handling.
    pub fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.fee_handling = fee_handling;
        self
    }

    /// Sets the maximum delegation for the validators bid during migration.
    pub fn with_maximum_delegation_amount(mut self, maximum_delegation_amount: u64) -> Self {
        self.maximum_delegation_amount = maximum_delegation_amount;
        self
    }

    /// Sets the minimum delegation for the validators bid during migration.
    pub fn with_minimum_delegation_amount(mut self, minimum_delegation_amount: u64) -> Self {
        self.minimum_delegation_amount = minimum_delegation_amount;
        self
    }

    /// Sets the enable entity flag.
    pub fn with_enable_addressable_entity(mut self, enable_entity: bool) -> Self {
        self.enable_addressable_entity = enable_entity;
        self
    }

    /// Consumes the `UpgradeRequestBuilder` and returns an [`ProtocolUpgradeConfig`].
    pub fn build(self) -> ProtocolUpgradeConfig {
        ProtocolUpgradeConfig::new(
            self.pre_state_hash,
            self.current_protocol_version,
            self.new_protocol_version,
            self.activation_point,
            self.new_gas_hold_handling,
            self.new_gas_hold_interval,
            self.new_validator_slots,
            self.new_auction_delay,
            self.new_locked_funds_period_millis,
            self.new_round_seigniorage_rate,
            self.new_unbonding_delay,
            self.global_state_update,
            self.chainspec_registry,
            self.fee_handling,
            self.maximum_delegation_amount,
            self.minimum_delegation_amount,
            self.enable_addressable_entity,
        )
    }
}

impl Default for UpgradeRequestBuilder {
    fn default() -> UpgradeRequestBuilder {
        UpgradeRequestBuilder {
            pre_state_hash: Default::default(),
            current_protocol_version: Default::default(),
            new_protocol_version: Default::default(),
            activation_point: None,
            new_gas_hold_handling: None,
            new_gas_hold_interval: None,
            new_validator_slots: None,
            new_auction_delay: None,
            new_locked_funds_period_millis: None,
            new_round_seigniorage_rate: None,
            new_unbonding_delay: None,
            global_state_update: Default::default(),
            chainspec_registry: ChainspecRegistry::new_with_optional_global_state(&[], None),
            fee_handling: FeeHandling::default(),
            maximum_delegation_amount: u64::MAX,
            minimum_delegation_amount: 0,
            enable_addressable_entity: false,
        }
    }
}
