use std::convert::{TryFrom, TryInto};

use casper_execution_engine::core::engine_state::step::{
    EvictItem, RewardItem, SlashItem, StepRequest,
};
use casper_types::{bytesrepr, bytesrepr::ToBytes, PublicKey};

use crate::engine_server::{
    ipc,
    mappings::{MappingError, ParsingError},
};

const PARENT_STATE_HASH: &str = "parent_state_hash";
const REWARD_ITEMS: &str = "reward_items";
const SLASH_ITEMS: &str = "slash_items";
const DISABLE_ITEMS: &str = "evict_items";
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

impl TryFrom<ipc::EvictItem> for EvictItem {
    type Error = MappingError;

    fn try_from(pb_evict_item: ipc::EvictItem) -> Result<Self, Self::Error> {
        let bytes: Vec<u8> = pb_evict_item
            .get_validator_id()
            .try_into()
            .map_err(|_| MappingError::Parsing(ParsingError(VALIDATOR_ID.to_string())))?;

        let validator_id: PublicKey =
            bytesrepr::deserialize(bytes).map_err(MappingError::Serialization)?;

        Ok(EvictItem::new(validator_id))
    }
}

impl TryFrom<EvictItem> for ipc::EvictItem {
    type Error = bytesrepr::Error;

    fn try_from(evict_item: EvictItem) -> Result<Self, Self::Error> {
        let mut result = ipc::EvictItem::new();
        let bytes = evict_item.validator_id.to_bytes()?;
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

        let evict_items = {
            let mut ret: Vec<EvictItem> = vec![];
            for item in pb_step_request.take_evict_items().into_iter() {
                let evict_item: EvictItem = item
                    .try_into()
                    .map_err(|_| MappingError::Parsing(ParsingError(DISABLE_ITEMS.to_string())))?;
                ret.push(evict_item);
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
            evict_items,
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

        let evict_items = {
            let mut ret: Vec<ipc::EvictItem> = vec![];
            for item in step_request.evict_items.into_iter() {
                let ipc = item.try_into()?;
                ret.push(ipc);
            }
            ret
        };
        result.set_evict_items(evict_items.into());

        Ok(result)
    }
}
