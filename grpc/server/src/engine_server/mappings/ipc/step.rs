use std::convert::{TryFrom, TryInto};

use casper_execution_engine::core::engine_state::step::{RewardItem, SlashItem, StepRequest};
use casper_types::U512;

use crate::engine_server::{
    ipc,
    mappings::{MappingError, ParsingError},
};

impl TryFrom<ipc::SlashItem> for SlashItem {
    type Error = MappingError;

    fn try_from(pb_slash_item: ipc::SlashItem) -> Result<Self, Self::Error> {
        let validator_id = pb_slash_item
            .get_validator_id()
            .try_into()
            .map_err(|_| MappingError::Parsing(ParsingError("validator_id".to_string())))?;
        Ok(SlashItem::new(validator_id))
    }
}

impl From<SlashItem> for ipc::SlashItem {
    fn from(slash_item: SlashItem) -> Self {
        let mut result = ipc::SlashItem::new();
        result.set_validator_id(slash_item.validator_id);
        result
    }
}

impl TryFrom<ipc::RewardItem> for RewardItem {
    type Error = MappingError;

    fn try_from(pb_reward_item: ipc::RewardItem) -> Result<Self, Self::Error> {
        let validator_id = pb_reward_item
            .get_validator_id()
            .try_into()
            .map_err(|_| MappingError::Parsing(ParsingError("validator_id".to_string())))?;
        let value: U512 = pb_reward_item.get_value().clone().try_into()?;

        Ok(RewardItem::new(validator_id, value))
    }
}

impl From<RewardItem> for ipc::RewardItem {
    fn from(reward_item: RewardItem) -> Self {
        let mut result = ipc::RewardItem::new();
        result.set_validator_id(reward_item.validator_id);
        result.set_value(reward_item.value.into());
        result
    }
}

impl TryFrom<ipc::StepRequest> for StepRequest {
    type Error = MappingError;

    fn try_from(mut pb_step_request: ipc::StepRequest) -> Result<Self, Self::Error> {
        let parent_state_hash = pb_step_request
            .get_parent_state_hash()
            .try_into()
            .map_err(|_| MappingError::InvalidStateHash("parent_state_hash".to_string()))?;

        let protocol_version = pb_step_request.take_protocol_version().into();

        let slash_items = {
            let mut ret: Vec<SlashItem> = vec![];
            for item in pb_step_request.take_slash_items().into_iter() {
                let slash_item: SlashItem = item
                    .try_into()
                    .map_err(|_| MappingError::Parsing(ParsingError("slash_items".to_string())))?;
                ret.push(slash_item);
            }
            ret
        };

        let reward_items = {
            let mut ret: Vec<RewardItem> = vec![];
            for item in pb_step_request.take_reward_items().into_iter() {
                let reward_item: RewardItem = item
                    .try_into()
                    .map_err(|_| MappingError::Parsing(ParsingError("reward_items".to_string())))?;
                ret.push(reward_item);
            }
            ret
        };

        Ok(StepRequest::new(
            parent_state_hash,
            protocol_version,
            slash_items,
            reward_items,
        ))
    }
}

impl From<StepRequest> for ipc::StepRequest {
    fn from(step_request: StepRequest) -> Self {
        let mut result = ipc::StepRequest::new();
        result.set_parent_state_hash(step_request.parent_state_hash.to_vec());
        result.set_protocol_version(step_request.protocol_version.into());

        result.set_slash_items(
            step_request
                .slash_items
                .into_iter()
                .map(|item| item.into())
                .collect(),
        );
        result.set_reward_items(
            step_request
                .reward_items
                .into_iter()
                .map(|item| item.into())
                .collect(),
        );
        result
    }
}

// impl From<Vec<SlashItem>> for ipc::
