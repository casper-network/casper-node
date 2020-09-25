use casper_engine_grpc_server::engine_server::{
    ipc::{
        ChainSpec_ActivationPoint, ChainSpec_UpgradePoint, ChainSpec_WasmConfig, DeployCode,
        UpgradeRequest,
    },
    state,
};
use casper_execution_engine::shared::wasm_config::WasmConfig;
use casper_types::ProtocolVersion;

pub struct UpgradeRequestBuilder {
    pre_state_hash: Vec<u8>,
    current_protocol_version: state::ProtocolVersion,
    new_protocol_version: state::ProtocolVersion,
    upgrade_installer: DeployCode,
    new_wasm_config: Option<ChainSpec_WasmConfig>,
    activation_point: ChainSpec_ActivationPoint,
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

    pub fn with_installer_code(mut self, upgrade_installer: DeployCode) -> Self {
        self.upgrade_installer = upgrade_installer;
        self
    }

    pub fn with_new_wasm_config(mut self, opcode_costs: WasmConfig) -> Self {
        self.new_wasm_config = Some(opcode_costs.into());
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
        }
    }
}
