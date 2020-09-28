use casper_engine_grpc_server::engine_server::{ipc, state};
use casper_types::{ProtocolVersion, U512};

#[derive(Debug)]
pub struct SlashItem {
    validator_id: Vec<u8>,
}

impl SlashItem {
    pub fn new(validator_id: Vec<u8>) -> Self {
        SlashItem { validator_id }
    }
}

impl Default for SlashItem {
    fn default() -> Self {
        SlashItem {
            validator_id: Default::default(),
        }
    }
}

impl From<SlashItem> for ipc::SlashItem {
    fn from(slash_item: SlashItem) -> Self {
        let mut item = ipc::SlashItem::new();
        item.set_validator_id(slash_item.validator_id);
        item
    }
}

#[derive(Debug)]
pub struct RewardItem {
    validator_id: Vec<u8>,
    value: U512,
}

#[allow(dead_code)]
impl RewardItem {
    pub fn new(validator_id: Vec<u8>, value: U512) -> Self {
        RewardItem {
            validator_id,
            value,
        }
    }
}

impl Default for RewardItem {
    fn default() -> Self {
        RewardItem {
            validator_id: Default::default(),
            value: Default::default(),
        }
    }
}

impl From<RewardItem> for ipc::RewardItem {
    fn from(reward_item: RewardItem) -> Self {
        let mut item = ipc::RewardItem::new();
        item.set_validator_id(reward_item.validator_id);
        item.set_value(reward_item.value.into());
        item
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
