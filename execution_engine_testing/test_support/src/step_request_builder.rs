use casper_execution_engine::core::engine_state::{
    step::{EvictItem, RewardItem, SlashItem},
    StepRequest,
};
use casper_hashing::Digest;
use casper_types::{EraId, ProtocolVersion};

/// Builder for creating a [`StepRequest`].
#[derive(Debug, Clone)]
pub struct StepRequestBuilder {
    parent_state_hash: Digest,
    protocol_version: ProtocolVersion,
    slash_items: Vec<SlashItem>,
    reward_items: Vec<RewardItem>,
    evict_items: Vec<EvictItem>,
    run_auction: bool,
    next_era_id: EraId,
    era_end_timestamp_millis: u64,
}

impl StepRequestBuilder {
    /// Returns a new `StepRequestBuilder`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets `parent_state_hash` to the given [`Digest`].
    pub fn with_parent_state_hash(mut self, parent_state_hash: Digest) -> Self {
        self.parent_state_hash = parent_state_hash;
        self
    }

    /// Sets `protocol_version` to the given [`ProtocolVersion`].
    pub fn with_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.protocol_version = protocol_version;
        self
    }

    /// Pushes the given [`SlashItem`] into `slash_items`.
    pub fn with_slash_item(mut self, slash_item: SlashItem) -> Self {
        self.slash_items.push(slash_item);
        self
    }

    /// Pushes the given [`RewardItem`] into `reward_items`.
    pub fn with_reward_item(mut self, reward_item: RewardItem) -> Self {
        self.reward_items.push(reward_item);
        self
    }

    /// Appends the given vector of [`RewardItem`] into `reward_items`.
    pub fn with_reward_items(mut self, reward_items: impl IntoIterator<Item = RewardItem>) -> Self {
        self.reward_items.extend(reward_items);
        self
    }

    /// Pushes the given [`EvictItem`] into `evict_items`.
    pub fn with_evict_item(mut self, evict_item: EvictItem) -> Self {
        self.evict_items.push(evict_item);
        self
    }

    /// Sets `run_auction`.
    pub fn with_run_auction(mut self, run_auction: bool) -> Self {
        self.run_auction = run_auction;
        self
    }

    /// Sets `next_era_id` to the given [`EraId`].
    pub fn with_next_era_id(mut self, next_era_id: EraId) -> Self {
        self.next_era_id = next_era_id;
        self
    }

    /// Sets `era_end_timestamp_millis`.
    pub fn with_era_end_timestamp_millis(mut self, era_end_timestamp_millis: u64) -> Self {
        self.era_end_timestamp_millis = era_end_timestamp_millis;
        self
    }

    /// Consumes the [`StepRequestBuilder`] and returns a [`StepRequest`].
    pub fn build(self) -> StepRequest {
        StepRequest::new(
            self.parent_state_hash,
            self.protocol_version,
            self.slash_items,
            self.reward_items,
            self.evict_items,
            self.next_era_id,
            self.era_end_timestamp_millis,
        )
    }
}

impl Default for StepRequestBuilder {
    fn default() -> Self {
        StepRequestBuilder {
            parent_state_hash: Default::default(),
            protocol_version: Default::default(),
            slash_items: Default::default(),
            reward_items: Default::default(),
            evict_items: Default::default(),
            run_auction: true, //<-- run_auction by default
            next_era_id: Default::default(),
            era_end_timestamp_millis: Default::default(),
        }
    }
}
