use casper_engine_grpc_server::engine_server::{ipc, state};
use casper_types::{bytesrepr, bytesrepr::ToBytes, ProtocolVersion, PublicKey};
use std::convert::{TryFrom, TryInto};

#[derive(Debug)]
pub struct SlashItem {
    validator_id: PublicKey,
}

impl SlashItem {
    pub fn new(validator_id: PublicKey) -> Self {
        SlashItem { validator_id }
    }
}

impl TryFrom<SlashItem> for ipc::SlashItem {
    type Error = bytesrepr::Error;

    fn try_from(slash_item: SlashItem) -> Result<Self, Self::Error> {
        let validator_id = slash_item.validator_id.to_bytes()?;
        let mut item = ipc::SlashItem::new();
        item.set_validator_id(validator_id);
        Ok(item)
    }
}

#[derive(Debug)]
pub struct RewardItem {
    validator_id: PublicKey,
    value: u64,
}

#[allow(dead_code)]
impl RewardItem {
    pub fn new(validator_id: PublicKey, value: u64) -> Self {
        RewardItem {
            validator_id,
            value,
        }
    }
}

impl TryFrom<RewardItem> for ipc::RewardItem {
    type Error = bytesrepr::Error;

    fn try_from(reward_item: RewardItem) -> Result<Self, Self::Error> {
        let validator_id = reward_item.validator_id.to_bytes()?;
        let mut item = ipc::RewardItem::new();
        item.set_validator_id(validator_id);
        item.set_value(reward_item.value);
        Ok(item)
    }
}

#[derive(Debug)]
pub struct StepRequestBuilder {
    state_root_hash: Vec<u8>,
    protocol_version: state::ProtocolVersion,
    slash_items: Vec<ipc::SlashItem>,
    reward_items: Vec<ipc::RewardItem>,
    run_auction: bool,
}

impl StepRequestBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_state_root_hash(mut self, state_root_hash: Vec<u8>) -> Self {
        self.state_root_hash = state_root_hash;
        self
    }

    pub fn with_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.protocol_version = protocol_version.into();
        self
    }

    pub fn with_slash_item(mut self, slash_item: SlashItem) -> Self {
        self.slash_items.push(slash_item.try_into().unwrap());
        self
    }

    pub fn with_reward_item(mut self, reward_item: RewardItem) -> Self {
        self.reward_items.push(reward_item.try_into().unwrap());
        self
    }

    pub fn with_run_auction(mut self, run_auction: bool) -> Self {
        self.run_auction = run_auction;
        self
    }

    pub fn build(self) -> ipc::StepRequest {
        let mut request = ipc::StepRequest::new();
        request.set_parent_state_hash(self.state_root_hash);
        request.set_protocol_version(self.protocol_version);
        request.set_slash_items(self.slash_items.into());
        request.set_reward_items(self.reward_items.into());
        request.set_run_auction(self.run_auction);
        request
    }
}

impl Default for StepRequestBuilder {
    fn default() -> Self {
        StepRequestBuilder {
            state_root_hash: Default::default(),
            protocol_version: Default::default(),
            slash_items: Default::default(),
            reward_items: Default::default(),
            run_auction: true, //<-- run_auction by default
        }
    }
}
