use std::collections::BTreeMap;

use num_rational::Ratio;

use casper_types::{
    ChainspecRegistry, Digest, EraId, FeeHandling, Key, ProtocolUpgradeConfig, ProtocolVersion,
    StoredValue,
};

/// Builds an `UpgradeConfig`.
pub struct UpgradeRequestBuilder {
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
    fee_handling: FeeHandling,
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

    /// Sets the Chainspec registry.
    pub fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.fee_handling = fee_handling;
        self
    }

    /// Consumes the `UpgradeRequestBuilder` and returns an [`ProtocolUpgradeConfig`].
    pub fn build(self) -> ProtocolUpgradeConfig {
        ProtocolUpgradeConfig::new(
            self.pre_state_hash,
            self.current_protocol_version,
            self.new_protocol_version,
            self.activation_point,
            self.new_validator_slots,
            self.new_auction_delay,
            self.new_locked_funds_period_millis,
            self.new_round_seigniorage_rate,
            self.new_unbonding_delay,
            self.global_state_update,
            self.chainspec_registry,
            self.fee_handling,
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
            new_validator_slots: None,
            new_auction_delay: None,
            new_locked_funds_period_millis: None,
            new_round_seigniorage_rate: None,
            new_unbonding_delay: None,
            global_state_update: Default::default(),
            chainspec_registry: ChainspecRegistry::new_with_optional_global_state(&[], None),
            fee_handling: FeeHandling::default(),
        }
    }
}
