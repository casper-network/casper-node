use casper_engine_grpc_server::engine_server::{ipc, state};
use casper_types::{ProtocolVersion, U512};

#[derive(Debug)]
pub struct StepItem {
    validator_id: Vec<u8>,
    value: U512,
}

impl StepItem {
    pub fn new(validator_id: Vec<u8>, value: U512) -> Self {
        StepItem {
            validator_id,
            value,
        }
    }

    pub fn as_slash_item(self) -> ipc::SlashItem {
        let mut item = ipc::SlashItem::new();
        item.set_validator_id(self.validator_id);
        item.set_value(self.value.into());
        item
    }

    pub fn as_reward_item(self) -> ipc::RewardItem {
        let mut item = ipc::RewardItem::new();
        item.set_validator_id(self.validator_id);
        item.set_value(self.value.into());
        item
    }
}

impl Default for StepItem {
    fn default() -> Self {
        StepItem {
            validator_id: Default::default(),
            value: Default::default(),
        }
    }
}

#[derive(Debug)]
pub struct StepRequestBuilder {
    parent_state_hash: Vec<u8>,
    protocol_version: state::ProtocolVersion,
    slash_items: Vec<ipc::SlashItem>,
    reward_items: Vec<ipc::RewardItem>,
}

impl StepRequestBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_parent_state_hash(mut self, parent_state_hash: Vec<u8>) -> Self {
        self.parent_state_hash = parent_state_hash;
        self
    }

    pub fn with_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.protocol_version = protocol_version.into();
        self
    }

    pub fn with_slash_item(mut self, slash_item: ipc::SlashItem) -> Self {
        self.slash_items.push(slash_item);
        self
    }

    pub fn with_reward_item(mut self, reward_item: ipc::RewardItem) -> Self {
        self.reward_items.push(reward_item);
        self
    }

    pub fn build(self) -> ipc::StepRequest {
        let mut request = ipc::StepRequest::new();
        request.set_parent_state_hash(self.parent_state_hash);
        request.set_protocol_version(self.protocol_version);
        request.set_slash_items(self.slash_items.into());
        request.set_reward_items(self.reward_items.into());
        request
    }
}

impl Default for StepRequestBuilder {
    fn default() -> Self {
        StepRequestBuilder {
            parent_state_hash: Default::default(),
            protocol_version: Default::default(),
            slash_items: Default::default(),
            reward_items: Default::default(),
        }
    }
}
