use std::convert::{TryFrom, TryInto};

use casper_execution_engine::core::engine_state::step::{
    DisableItem, RewardItem, SlashItem, StepRequest,
};
use casper_types::{bytesrepr, bytesrepr::ToBytes, PublicKey};

use crate::engine_server::{
    ipc,
    mappings::{MappingError, ParsingError},
};

const PARENT_STATE_HASH: &str = "parent_state_hash";
const REWARD_ITEMS: &str = "reward_items";
const SLASH_ITEMS: &str = "slash_items";
const DISABLE_ITEMS: &str = "disable_items";
const VALIDATOR_ID: &str = "validator_id";

impl TryFrom<ipc::SlashItem> for SlashItem {
    type Error = MappingError;

    fn try_from(pb_slash_item: ipc::SlashItem) -> Result<Self, Self::Error> {
        let bytes: Vec<u8> = pb_slash_item
            .get_validator_id()
            .try_into()
            .map_err(|_| MappingError::Parsing(ParsingError(VALIDATOR_ID.to_string())))?;

        let validator_id: PublicKey =
            bytesrepr::deserialize(bytes).map_err(MappingError::Serialization)?;

        Ok(SlashItem::new(validator_id))
    }
}

impl TryFrom<SlashItem> for ipc::SlashItem {
    type Error = bytesrepr::Error;

    fn try_from(slash_item: SlashItem) -> Result<Self, Self::Error> {
        let mut result = ipc::SlashItem::new();
        let bytes = slash_item.validator_id.to_bytes()?;
        result.set_validator_id(bytes);
        Ok(result)
    }
}

impl TryFrom<ipc::RewardItem> for RewardItem {
    type Error = MappingError;

    fn try_from(pb_reward_item: ipc::RewardItem) -> Result<Self, Self::Error> {
        let bytes: Vec<u8> = pb_reward_item
            .get_validator_id()
            .try_into()
            .map_err(|_| MappingError::Parsing(ParsingError(VALIDATOR_ID.to_string())))?;

        let validator_id: PublicKey =
            bytesrepr::deserialize(bytes).map_err(MappingError::Serialization)?;
        let value: u64 = pb_reward_item.get_value();

        Ok(RewardItem::new(validator_id, value))
    }
}

impl TryFrom<RewardItem> for ipc::RewardItem {
    type Error = bytesrepr::Error;

    fn try_from(reward_item: RewardItem) -> Result<Self, Self::Error> {
        let mut result = ipc::RewardItem::new();
        let bytes = reward_item.validator_id.to_bytes()?;
        result.set_validator_id(bytes);
        Ok(result)
    }
}

impl TryFrom<ipc::DisableItem> for DisableItem {
    type Error = MappingError;

    fn try_from(pb_disable_item: ipc::DisableItem) -> Result<Self, Self::Error> {
        let bytes: Vec<u8> = pb_disable_item
            .get_validator_id()
            .try_into()
            .map_err(|_| MappingError::Parsing(ParsingError(VALIDATOR_ID.to_string())))?;

        let validator_id: PublicKey =
            bytesrepr::deserialize(bytes).map_err(MappingError::Serialization)?;

        Ok(DisableItem::new(validator_id))
    }
}

impl TryFrom<DisableItem> for ipc::DisableItem {
    type Error = bytesrepr::Error;

    fn try_from(disable_item: DisableItem) -> Result<Self, Self::Error> {
        let mut result = ipc::DisableItem::new();
        let bytes = disable_item.validator_id.to_bytes()?;
        result.set_validator_id(bytes);
        Ok(result)
    }
}

impl TryFrom<ipc::StepRequest> for StepRequest {
    type Error = MappingError;

    fn try_from(mut pb_step_request: ipc::StepRequest) -> Result<Self, Self::Error> {
        let parent_state_hash = pb_step_request
            .get_parent_state_hash()
            .try_into()
            .map_err(|_| MappingError::InvalidStateHash(PARENT_STATE_HASH.to_string()))?;

        let protocol_version = pb_step_request.take_protocol_version().into();

        let slash_items = {
            let mut ret: Vec<SlashItem> = vec![];
            for item in pb_step_request.take_slash_items().into_iter() {
                let slash_item: SlashItem = item
                    .try_into()
                    .map_err(|_| MappingError::Parsing(ParsingError(SLASH_ITEMS.to_string())))?;
                ret.push(slash_item);
            }
            ret
        };

        let reward_items = {
            let mut ret: Vec<RewardItem> = vec![];
            for item in pb_step_request.take_reward_items().into_iter() {
                let reward_item: RewardItem = item
                    .try_into()
                    .map_err(|_| MappingError::Parsing(ParsingError(REWARD_ITEMS.to_string())))?;
                ret.push(reward_item);
            }
            ret
        };

        let disable_items = {
            let mut ret: Vec<DisableItem> = vec![];
            for item in pb_step_request.take_disable_items().into_iter() {
                let disable_item: DisableItem = item
                    .try_into()
                    .map_err(|_| MappingError::Parsing(ParsingError(DISABLE_ITEMS.to_string())))?;
                ret.push(disable_item);
            }
            ret
        };

        let run_auction = pb_step_request.get_run_auction();

        let next_era_id = pb_step_request.get_next_era_id();

        Ok(StepRequest::new(
            parent_state_hash,
            protocol_version,
            slash_items,
            reward_items,
            disable_items,
            run_auction,
            next_era_id,
        ))
    }
}

impl TryFrom<StepRequest> for ipc::StepRequest {
    type Error = bytesrepr::Error;

    fn try_from(step_request: StepRequest) -> Result<Self, Self::Error> {
        let mut result = ipc::StepRequest::new();
        result.set_parent_state_hash(step_request.pre_state_hash.to_vec());
        result.set_protocol_version(step_request.protocol_version.into());

        let slash_items = {
            let mut ret: Vec<ipc::SlashItem> = vec![];
            for item in step_request.slash_items.into_iter() {
                let ipc = item.try_into()?;
                ret.push(ipc);
            }
            ret
        };
        result.set_slash_items(slash_items.into());

        let reward_items = {
            let mut ret: Vec<ipc::RewardItem> = vec![];
            for item in step_request.reward_items.into_iter() {
                let ipc = item.try_into()?;
                ret.push(ipc);
            }
            ret
        };
        result.set_reward_items(reward_items.into());

        let disable_items = {
            let mut ret: Vec<ipc::DisableItem> = vec![];
            for item in step_request.disable_items.into_iter() {
                let ipc = item.try_into()?;
                ret.push(ipc);
            }
            ret
        };
        result.set_disable_items(disable_items.into());

        Ok(result)
    }
}
