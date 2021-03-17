use std::collections::BTreeMap;

use num_rational::Ratio;

use casper_execution_engine::{
    core::engine_state::{upgrade::ActivationPoint, UpgradeConfig},
    shared::{
        newtypes::Blake2bHash, stored_value::StoredValue, system_config::SystemConfig,
        wasm_config::WasmConfig,
    },
};
use casper_types::{Key, ProtocolVersion};

#[derive(Default)]
pub struct UpgradeRequestBuilder {
    pre_state_hash: Blake2bHash,
    current_protocol_version: ProtocolVersion,
    new_protocol_version: ProtocolVersion,
    new_wasm_config: Option<WasmConfig>,
    new_system_config: Option<SystemConfig>,
    activation_point: Option<ActivationPoint>,
    new_validator_slots: Option<u32>,
    new_auction_delay: Option<u64>,
    new_locked_funds_period_millis: Option<u64>,
    new_round_seigniorage_rate: Option<Ratio<u64>>,
    new_unbonding_delay: Option<u64>,
    global_state_update: BTreeMap<Key, StoredValue>,
}

impl UpgradeRequestBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_pre_state_hash(mut self, pre_state_hash: Blake2bHash) -> Self {
        self.pre_state_hash = pre_state_hash;
        self
    }

    pub fn with_current_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.current_protocol_version = protocol_version;
        self
    }

    pub fn with_new_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.new_protocol_version = protocol_version;
        self
    }

    pub fn with_new_validator_slots(mut self, new_validator_slots: u32) -> Self {
        self.new_validator_slots = Some(new_validator_slots);
        self
    }

    pub fn with_new_wasm_config(mut self, opcode_costs: WasmConfig) -> Self {
        self.new_wasm_config = Some(opcode_costs);
        self
    }
    pub fn with_new_auction_delay(mut self, new_auction_delay: u64) -> Self {
        self.new_auction_delay = Some(new_auction_delay);
        self
    }

    pub fn with_new_locked_funds_period_millis(
        mut self,
        new_locked_funds_period_millis: u64,
    ) -> Self {
        self.new_locked_funds_period_millis = Some(new_locked_funds_period_millis);
        self
    }

    pub fn with_new_round_seigniorage_rate(mut self, rate: Ratio<u64>) -> Self {
        self.new_round_seigniorage_rate = Some(rate);
        self
    }

    pub fn with_new_unbonding_delay(mut self, unbonding_delay: u64) -> Self {
        self.new_unbonding_delay = Some(unbonding_delay);
        self
    }

    pub fn with_new_system_config(mut self, new_system_config: SystemConfig) -> Self {
        self.new_system_config = Some(new_system_config);
        self
    }

    pub fn with_global_state_update(
        mut self,
        global_state_update: BTreeMap<Key, StoredValue>,
    ) -> Self {
        self.global_state_update = global_state_update;
        self
    }

    pub fn with_activation_point(mut self, height: ActivationPoint) -> Self {
        self.activation_point = Some(height);
        self
    }

    pub fn build(self) -> UpgradeConfig {
        UpgradeConfig::new(
            self.pre_state_hash,
            self.current_protocol_version,
            self.new_protocol_version,
            self.new_wasm_config,
            self.new_system_config,
            self.activation_point,
            self.new_validator_slots,
            self.new_auction_delay,
            self.new_locked_funds_period_millis,
            self.new_round_seigniorage_rate,
            self.new_unbonding_delay,
            self.global_state_update,
        )
    }
}
