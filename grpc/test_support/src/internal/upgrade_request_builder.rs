use casper_engine_grpc_server::engine_server::{
    ipc::{
        ChainSpec_ActivationPoint, ChainSpec_NewAuctionDelay, ChainSpec_NewInitialEraId,
        ChainSpec_NewLockedFundsPeriod, ChainSpec_NewValidatorSlots, ChainSpec_UpgradePoint,
        ChainSpec_WasmConfig, DeployCode, UpgradeRequest,
    },
    state,
};
use casper_execution_engine::shared::wasm_config::WasmConfig;
use casper_types::{auction::EraId, ProtocolVersion};

pub struct UpgradeRequestBuilder {
    pre_state_hash: Vec<u8>,
    current_protocol_version: state::ProtocolVersion,
    new_protocol_version: state::ProtocolVersion,
    upgrade_installer: DeployCode,
    new_wasm_config: Option<ChainSpec_WasmConfig>,
    activation_point: ChainSpec_ActivationPoint,
    new_validator_slots: Option<u32>,
    new_auction_delay: Option<u64>,
    new_initial_era_id: Option<EraId>,
    new_locked_funds_period: Option<EraId>,
}

impl UpgradeRequestBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_pre_state_hash(mut self, pre_state_hash: &[u8]) -> Self {
        self.pre_state_hash = pre_state_hash.to_vec();
        self
    }

    pub fn with_current_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.current_protocol_version = protocol_version.into();
        self
    }

    pub fn with_new_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.new_protocol_version = protocol_version.into();
        self
    }

    pub fn with_new_validator_slots(mut self, new_validator_slots: u32) -> Self {
        self.new_validator_slots = Some(new_validator_slots);
        self
    }

    pub fn with_installer_code(mut self, upgrade_installer: DeployCode) -> Self {
        self.upgrade_installer = upgrade_installer;
        self
    }

    pub fn with_new_wasm_config(mut self, opcode_costs: WasmConfig) -> Self {
        self.new_wasm_config = Some(opcode_costs.into());
        self
    }

    pub fn with_new_auction_delay(mut self, new_auction_delay: u64) -> Self {
        self.new_auction_delay = Some(new_auction_delay);
        self
    }

    pub fn with_new_initial_era_id(mut self, new_initial_era_id: EraId) -> Self {
        self.new_initial_era_id = Some(new_initial_era_id);
        self
    }

    pub fn with_new_locked_funds_period(mut self, new_locked_funds_period: EraId) -> Self {
        self.new_locked_funds_period = Some(new_locked_funds_period);
        self
    }

    pub fn with_activation_point(mut self, rank: u64) -> Self {
        self.activation_point = {
            let mut ret = ChainSpec_ActivationPoint::new();
            ret.set_rank(rank);
            ret
        };
        self
    }

    pub fn build(self) -> UpgradeRequest {
        let mut upgrade_point = ChainSpec_UpgradePoint::new();
        upgrade_point.set_activation_point(self.activation_point);
        if let Some(new_wasm_config) = self.new_wasm_config {
            upgrade_point.set_new_wasm_config(new_wasm_config)
        }
        match self.new_validator_slots {
            None => {}
            Some(new_validator_slots) => {
                let mut chainspec_new_validator_slots = ChainSpec_NewValidatorSlots::new();
                chainspec_new_validator_slots.set_new_validator_slots(new_validator_slots);
                upgrade_point.set_new_validator_slots(chainspec_new_validator_slots);
            }
        }

        match self.new_auction_delay {
            None => {}
            Some(new_auction_delay) => {
                let mut chainspec_new_auction_delay = ChainSpec_NewAuctionDelay::new();
                chainspec_new_auction_delay.set_new_auction_delay(new_auction_delay);
                upgrade_point.set_new_auction_delay(chainspec_new_auction_delay);
            }
        }

        match self.new_initial_era_id {
            None => {}
            Some(new_initial_era_id) => {
                let mut chainspec_new_initial_era_id = ChainSpec_NewInitialEraId::new();
                chainspec_new_initial_era_id.set_new_initial_era_id(new_initial_era_id);
                upgrade_point.set_new_initial_era_id(chainspec_new_initial_era_id);
            }
        }

        match self.new_locked_funds_period {
            None => {}
            Some(new_locked_funds_period) => {
                let mut chainspec_new_locked_funds_period = ChainSpec_NewLockedFundsPeriod::new();
                chainspec_new_locked_funds_period
                    .set_new_locked_funds_period(new_locked_funds_period);
                upgrade_point.set_new_locked_funds_period(chainspec_new_locked_funds_period);
            }
        }

        upgrade_point.set_protocol_version(self.new_protocol_version);
        upgrade_point.set_upgrade_installer(self.upgrade_installer);

        let mut upgrade_request = UpgradeRequest::new();
        upgrade_request.set_protocol_version(self.current_protocol_version);
        upgrade_request.set_upgrade_point(upgrade_point);
        upgrade_request
    }
}

impl Default for UpgradeRequestBuilder {
    fn default() -> Self {
        UpgradeRequestBuilder {
            pre_state_hash: Default::default(),
            current_protocol_version: Default::default(),
            new_protocol_version: Default::default(),
            upgrade_installer: Default::default(),
            new_wasm_config: None,
            activation_point: Default::default(),
            new_validator_slots: Default::default(),
            new_auction_delay: Default::default(),
            new_initial_era_id: Default::default(),
            new_locked_funds_period: Default::default(),
        }
    }
}
