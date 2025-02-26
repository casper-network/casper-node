#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, ops::Range, sync::Arc};

use casper_storage::{
    data_access_layer::{
        DataAccessLayer, EraValidatorsRequest, RoundSeigniorageRateRequest,
        RoundSeigniorageRateResult, TotalSupplyRequest, TotalSupplyResult,
    },
    global_state::state::{lmdb::LmdbGlobalState, StateProvider},
};
use futures::stream::{self, StreamExt as _, TryStreamExt as _};

use itertools::Itertools;
use num_rational::Ratio;
use num_traits::{CheckedAdd, CheckedMul, ToPrimitive};
use thiserror::Error;
use tracing::trace;

use crate::{
    contract_runtime::metrics::Metrics,
    effect::{
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder,
    },
    types::ExecutableBlock,
};
use casper_types::{
    Block, Chainspec, CoreConfig, Digest, EraId, ProtocolVersion, PublicKey, RewardedSignatures,
    U512,
};

pub(crate) trait ReactorEventT:
    Send + From<StorageRequest> + From<ContractRuntimeRequest>
{
}

impl<T> ReactorEventT for T where T: Send + From<StorageRequest> + From<ContractRuntimeRequest> {}

#[derive(Debug)]
pub(crate) struct CitedBlock {
    protocol_version: ProtocolVersion,
    height: u64,
    era_id: EraId,
    proposer: PublicKey,
    rewarded_signatures: RewardedSignatures,
    state_root_hash: Digest,
    is_switch_block: bool,
    is_genesis: bool,
}

impl CitedBlock {
    fn from_executable_block(block: ExecutableBlock, protocol_version: ProtocolVersion) -> Self {
        Self {
            protocol_version,
            era_id: block.era_id,
            height: block.height,
            proposer: *block.proposer,
            rewarded_signatures: block.rewarded_signatures,
            state_root_hash: Digest::default(),
            is_switch_block: block.era_report.is_some(),
            is_genesis: block.era_id.is_genesis(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct RewardsInfo {
    eras_info: BTreeMap<EraId, EraInfo>,
    cited_blocks: Vec<CitedBlock>,
    cited_block_height_start: u64,
}

/// The era information needed in the rewards computation:
#[derive(Debug, Clone)]
pub(crate) struct EraInfo {
    weights: BTreeMap<PublicKey, U512>,
    total_weights: U512,
    reward_per_round: Ratio<U512>,
}

#[derive(Error, Debug)]
pub enum RewardsError {
    /// We got a block height which is not in the era range it should be in (should not happen).
    #[error("block height {0} is not in the era range")]
    HeightNotInEraRange(u64),
    /// The era is not in the range we have (should not happen).
    #[error("era {0} is not in the era range")]
    EraIdNotInEraRange(EraId),
    /// The validator public key is not in the era it should be in (should not happen).
    #[error("validator key {0:?} is not in the era")]
    ValidatorKeyNotInEra(Box<PublicKey>),
    /// We didn't have a required switch block.
    #[error("missing switch block for era {0}")]
    MissingSwitchBlock(EraId),
    /// We got an overflow while computing something.
    #[error("arithmetic overflow")]
    ArithmeticOverflow,
    #[error("failed to fetch block with height {0}")]
    FailedToFetchBlockWithHeight(u64),
    #[error("failed to fetch era {0}")]
    FailedToFetchEra(String),
    /// Fetching the era validators succedeed, but no info is present (should not happen).
    /// The `Digest` is the one that was queried.
    #[error("failed to fetch era validators for {0}")]
    FailedToFetchEraValidators(Digest),
    #[error("failed to fetch total supply")]
    FailedToFetchTotalSupply,
    #[error("failed to fetch seigniorage rate")]
    FailedToFetchSeigniorageRate,
}

impl RewardsInfo {
    pub async fn new<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
        protocol_version: ProtocolVersion,
        activation_era_id: EraId,
        maybe_upgraded_validators: Option<&BTreeMap<PublicKey, U512>>,
        signature_rewards_max_delay: u64,
        executable_block: ExecutableBlock,
    ) -> Result<Self, RewardsError> {
        let current_era_id = executable_block.era_id;
        // All the blocks that may appear as a signed block. They are collected upfront, so that we
        // don't have to worry about doing it one by one later.
        //
        // They are sorted from the oldest to the newest:

        let cited_block_height_start = {
            let previous_era_id = current_era_id.saturating_sub(1);
            let previous_era_switch_block_header = effect_builder
                .get_switch_block_header_by_era_id_from_storage(previous_era_id)
                .await
                .ok_or(RewardsError::MissingSwitchBlock(previous_era_id))?;

            if previous_era_id.is_genesis() || previous_era_id == activation_era_id {
                // We do not attempt to reward blocks from before an upgrade!
                previous_era_switch_block_header.height()
            } else {
                // Here we do not substract 1, because we want one block more:
                previous_era_switch_block_header
                    .height()
                    .saturating_sub(signature_rewards_max_delay)
            }
        };

        // We need just one block from before the upgrade to determine the validators in
        // the following era.
        let range_to_fetch = cited_block_height_start.saturating_sub(1)..executable_block.height;
        let mut cited_blocks =
            collect_past_blocks_batched(effect_builder, range_to_fetch.clone()).await?;

        tracing::info!(
            current_era_id = %current_era_id.value(),
            range_requested = ?range_to_fetch,
            num_fetched_blocks = %cited_blocks.len(),
            "blocks fetched",
        );

        let eras_info = Self::create_eras_info(
            data_access_layer,
            activation_era_id,
            current_era_id,
            maybe_upgraded_validators,
            cited_blocks.iter(),
        )?;

        cited_blocks.push(CitedBlock::from_executable_block(
            executable_block,
            protocol_version,
        ));

        Ok(RewardsInfo {
            eras_info,
            cited_blocks,
            cited_block_height_start,
        })
    }

    #[cfg(test)]
    pub fn new_testing(eras_info: BTreeMap<EraId, EraInfo>, cited_blocks: Vec<CitedBlock>) -> Self {
        let cited_block_height_start = cited_blocks.first().map(|block| block.height).unwrap_or(0);
        Self {
            eras_info,
            cited_blocks,
            cited_block_height_start,
        }
    }

    /// `block_hashs` is an iterator over the era ID to get the information about + the block
    /// hash to query to have such information (which may not be from the same era).
    fn create_eras_info<'a>(
        data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
        activation_era_id: EraId,
        current_era_id: EraId,
        maybe_upgraded_validators: Option<&BTreeMap<PublicKey, U512>>,
        mut cited_blocks: impl Iterator<Item = &'a CitedBlock>,
    ) -> Result<BTreeMap<EraId, EraInfo>, RewardsError> {
        let oldest_block = cited_blocks.next();

        // If the oldest block is genesis, we add the validator information for genesis (era 0) from
        // era 1, because it's the same:
        let oldest_block_is_genesis = oldest_block.is_some_and(|block| block.is_genesis);

        // Here, we gather a list of all of the era ID we need to fetch to calculate the rewards,
        // as well as the state root hash allowing to query this information.
        //
        // To get all of the needed era IDs, we take the very first block, then every switch block
        // We take the first block, because we need it for the first cited era, then every switch
        // block for every subsequent eras.
        // If the first block is itself a switch block, that's fine, because we fetch one block more
        // in the first place to handle this case.
        let eras_and_state_root_hashes: Vec<_> = oldest_block
            .into_iter()
            .chain(cited_blocks.filter(|&block| block.is_switch_block))
            .map(|block| {
                let state_root_hash = block.state_root_hash;
                let protocol_version = block.protocol_version;
                let era = if block.is_switch_block {
                    block.era_id.successor()
                } else {
                    block.era_id
                };
                (era, protocol_version, state_root_hash)
            })
            .collect();

        let num_eras_to_fetch =
            eras_and_state_root_hashes.len() + usize::from(oldest_block_is_genesis);

        let data_access_layer = &data_access_layer;

        let mut eras_info: BTreeMap<_, _> = eras_and_state_root_hashes
            .into_iter()
            .map(|(era_id, protocol_version, state_root_hash)| {
                let weights = if let (true, Some(upgraded_validators)) =
                    (era_id == activation_era_id, maybe_upgraded_validators)
                {
                    upgraded_validators.clone()
                } else {
                    let request = EraValidatorsRequest::new(state_root_hash);
                    let era_validators_result = data_access_layer.era_validators(request);
                    let msg = format!("{}", era_validators_result);
                    era_validators_result
                        .take_era_validators()
                        .ok_or(msg)
                        .map_err(RewardsError::FailedToFetchEra)?
                        // We consume the map to not clone the value:
                        .into_iter()
                        .find(|(key, _)| key == &era_id)
                        .ok_or(RewardsError::FailedToFetchEraValidators(state_root_hash))?
                        .1
                };

                let total_supply_request =
                    TotalSupplyRequest::new(state_root_hash, protocol_version);
                let total_supply = match data_access_layer.total_supply(total_supply_request) {
                    TotalSupplyResult::RootNotFound
                    | TotalSupplyResult::MintNotFound
                    | TotalSupplyResult::ValueNotFound(_)
                    | TotalSupplyResult::Failure(_) => {
                        return Err(RewardsError::FailedToFetchTotalSupply)
                    }
                    TotalSupplyResult::Success { total_supply } => total_supply,
                };

                let seigniorage_rate_request =
                    RoundSeigniorageRateRequest::new(state_root_hash, protocol_version);
                let seigniorage_rate =
                    match data_access_layer.round_seigniorage_rate(seigniorage_rate_request) {
                        RoundSeigniorageRateResult::RootNotFound
                        | RoundSeigniorageRateResult::MintNotFound
                        | RoundSeigniorageRateResult::ValueNotFound(_)
                        | RoundSeigniorageRateResult::Failure(_) => {
                            return Err(RewardsError::FailedToFetchSeigniorageRate);
                        }
                        RoundSeigniorageRateResult::Success { rate } => rate,
                    };

                let reward_per_round = seigniorage_rate * total_supply;
                let total_weights = weights.values().copied().sum();

                Ok::<_, RewardsError>((
                    era_id,
                    EraInfo {
                        weights,
                        total_weights,
                        reward_per_round,
                    },
                ))
            })
            .try_collect()?;

        // We cannot get the genesis info from a root hash, so we copy it from era 1 when needed.
        if oldest_block_is_genesis {
            let era_1 = EraId::from(1);
            let era_1_info = eras_info
                .get(&era_1)
                .ok_or(RewardsError::EraIdNotInEraRange(era_1))?;
            eras_info.insert(EraId::from(0), era_1_info.clone());
        }

        {
            let era_ids: Vec<_> = eras_info.keys().map(|id| id.value()).collect();
            tracing::info!(
                current_era_id = %current_era_id.value(),
                %num_eras_to_fetch,
                eras_fetched = ?era_ids,
            );
        }

        Ok(eras_info)
    }

    /// Returns the validators from a given era.
    pub fn validator_keys(
        &self,
        era_id: EraId,
    ) -> Result<impl Iterator<Item = PublicKey> + '_, RewardsError> {
        let keys = self
            .eras_info
            .get(&era_id)
            .ok_or(RewardsError::EraIdNotInEraRange(era_id))?
            .weights
            .keys()
            .cloned();

        Ok(keys)
    }

    /// Returns the total potential reward per block.
    /// Since it is per block, we do not care about the expected number of blocks per era.
    pub fn reward(&self, era_id: EraId) -> Result<Ratio<U512>, RewardsError> {
        Ok(self
            .eras_info
            .get(&era_id)
            .ok_or(RewardsError::EraIdNotInEraRange(era_id))?
            .reward_per_round)
    }

    /// Returns the weight ratio for a given validator for a given era.
    pub fn weight_ratio(
        &self,
        era_id: EraId,
        validator: &PublicKey,
    ) -> Result<Ratio<U512>, RewardsError> {
        let era = self
            .eras_info
            .get(&era_id)
            .ok_or(RewardsError::EraIdNotInEraRange(era_id))?;
        let weight = era
            .weights
            .get(validator)
            .ok_or_else(|| RewardsError::ValidatorKeyNotInEra(Box::new(validator.clone())))?;

        Ok(Ratio::new(*weight, era.total_weights))
    }

    /// Returns the era in which is the given block height.
    pub fn era_for_block_height(&self, height: u64) -> Result<EraId, RewardsError> {
        self.cited_blocks
            .iter()
            .find_map(|block| (block.height == height).then_some(block.era_id))
            .ok_or(RewardsError::HeightNotInEraRange(height))
    }

    /// Returns all the blocks belonging to an era.
    pub fn blocks_from_era(&self, era_id: EraId) -> impl Iterator<Item = &CitedBlock> {
        self.cited_blocks
            .iter()
            .filter(move |block| block.era_id == era_id)
    }
}

impl EraInfo {
    #[cfg(test)]
    pub fn new_testing(weights: BTreeMap<PublicKey, U512>, reward_per_round: Ratio<U512>) -> Self {
        let total_weights = weights.values().copied().sum();
        Self {
            weights,
            total_weights,
            reward_per_round,
        }
    }
}

/// First create the `RewardsInfo` structure, then compute the rewards.
/// It is done in 2 steps so that it is easier to unit test the rewards calculation.
pub(crate) async fn fetch_data_and_calculate_rewards_for_era<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
    chainspec: &Chainspec,
    metrics: &Arc<Metrics>,
    executable_block: ExecutableBlock,
) -> Result<BTreeMap<PublicKey, Vec<U512>>, RewardsError> {
    let current_era_id = executable_block.era_id;
    tracing::info!(
        current_era_id = %current_era_id.value(),
        "starting the rewards calculation"
    );

    if current_era_id.is_genesis()
        || current_era_id == chainspec.protocol_config.activation_point.era_id()
    {
        // Special case: genesis block and immediate switch blocks do not yield any reward, because
        // there is no block producer, and no signatures from previous blocks to be rewarded:
        Ok(BTreeMap::new())
    } else {
        let rewards_info = RewardsInfo::new(
            effect_builder,
            data_access_layer,
            chainspec.protocol_version(),
            chainspec.protocol_config.activation_point.era_id(),
            chainspec
                .protocol_config
                .global_state_update
                .as_ref()
                .and_then(|gsu| gsu.validators.as_ref()),
            chainspec.core_config.signature_rewards_max_delay,
            executable_block,
        )
        .await?;

        let cited_blocks_count_current_era = rewards_info.blocks_from_era(current_era_id).count();

        let reward_per_round_current_era = rewards_info
            .eras_info
            .get(&current_era_id)
            .expect("expected EraInfo")
            .reward_per_round;

        let rewards = rewards_for_era(rewards_info, current_era_id, &chainspec.core_config);

        // Calculate and push reward metric(s)
        if let Ok(rewards_map) = &rewards {
            let expected_total_seigniorage = reward_per_round_current_era
                .to_integer()
                .saturating_mul(U512::from(cited_blocks_count_current_era as u64));
            let actual_total_seigniorage =
                rewards_map
                    .iter()
                    .fold(U512::zero(), |acc, (_, rewards_vec)| {
                        let current_era_reward = rewards_vec
                            .first()
                            .expect("expected current era reward amount");
                        acc.saturating_add(*current_era_reward)
                    });
            let seigniorage_target_fraction = Ratio::new(
                actual_total_seigniorage.low_u128(),
                expected_total_seigniorage.low_u128(),
            );
            let gauge_value = match Ratio::to_f64(&seigniorage_target_fraction) {
                Some(v) => v,
                None => f64::NAN,
            };
            metrics.seigniorage_target_fraction.set(gauge_value)
        }

        rewards
    }
}

pub(crate) fn rewards_for_era(
    rewards_info: RewardsInfo,
    current_era_id: EraId,
    core_config: &CoreConfig,
) -> Result<BTreeMap<PublicKey, Vec<U512>>, RewardsError> {
    fn to_ratio_u512(ratio: Ratio<u64>) -> Ratio<U512> {
        Ratio::new(U512::from(*ratio.numer()), U512::from(*ratio.denom()))
    }

    let ratio_u512_zero = Ratio::new(U512::zero(), U512::one());
    let zero_for_current_era = {
        let mut map = BTreeMap::new();
        map.insert(current_era_id, ratio_u512_zero);
        map
    };
    let mut full_reward_for_validators: BTreeMap<_, _> = rewards_info
        .validator_keys(current_era_id)?
        .map(|key| (key, zero_for_current_era.clone()))
        .collect();

    let mut increase_value_for_key_and_era =
        |key: PublicKey, era: EraId, value: Ratio<U512>| -> Result<(), RewardsError> {
            match full_reward_for_validators.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let mut map = BTreeMap::new();
                    map.insert(era, value);
                    entry.insert(map);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let old_value = entry.get().get(&era).unwrap_or(&ratio_u512_zero);
                    let new_value = old_value
                        .checked_add(&value)
                        .ok_or(RewardsError::ArithmeticOverflow)?;
                    entry.get_mut().insert(era, new_value);
                }
            }

            Ok(())
        };

    // Rules out a special case: genesis block does not yield any reward,
    // because there is no block producer, and no previous blocks whose
    // signatures are to be rewarded:
    debug_assert!(
        current_era_id.is_genesis() == false,
        "the genesis block should be handled as a special case"
    );

    let collection_proportion = to_ratio_u512(core_config.collection_rewards_proportion());
    let contribution_proportion = to_ratio_u512(core_config.contribution_rewards_proportion());

    // Reward for producing a block from this era:
    let production_reward = to_ratio_u512(core_config.production_rewards_proportion())
        .checked_mul(&rewards_info.reward(current_era_id)?)
        .ok_or(RewardsError::ArithmeticOverflow)?;

    // Collect all rewards as a ratio:
    for block in rewards_info.blocks_from_era(current_era_id) {
        // Transfer the block production reward for this block proposer:
        trace!(
            proposer=?block.proposer,
            amount=%production_reward.to_integer(),
            block=%block.height,
            "proposer reward"
        );
        increase_value_for_key_and_era(block.proposer.clone(), current_era_id, production_reward)?;

        // Now, let's compute the reward attached to each signed block reported by the block
        // we examine:
        for (signature_rewards, signed_block_height) in block
            .rewarded_signatures
            .iter()
            .zip((rewards_info.cited_block_height_start..block.height).rev())
        {
            let signed_block_era = rewards_info.era_for_block_height(signed_block_height)?;
            let validators_providing_signature =
                signature_rewards.to_validator_set(rewards_info.validator_keys(signed_block_era)?);

            for signing_validator in validators_providing_signature {
                // Reward for contributing to the finality signature, ie signing this block:
                let contribution_reward = rewards_info
                    .weight_ratio(signed_block_era, &signing_validator)?
                    .checked_mul(&contribution_proportion)
                    .ok_or(RewardsError::ArithmeticOverflow)?
                    .checked_mul(&rewards_info.reward(signed_block_era)?)
                    .ok_or(RewardsError::ArithmeticOverflow)?;
                // Reward for gathering this signature. It is both weighted by the block
                // producing/signature collecting validator, and the signing validator:
                let collection_reward = rewards_info
                    .weight_ratio(signed_block_era, &signing_validator)?
                    .checked_mul(&collection_proportion)
                    .ok_or(RewardsError::ArithmeticOverflow)?
                    .checked_mul(&rewards_info.reward(signed_block_era)?)
                    .ok_or(RewardsError::ArithmeticOverflow)?;

                trace!(
                    signer=?signing_validator,
                    amount=%contribution_reward.to_integer(),
                    block=%block.height,
                    signed_block=%signed_block_height,
                    "signature contribution reward"
                );
                trace!(
                    collector=?block.proposer,
                    signer=?signing_validator,
                    amount=%collection_reward.to_integer(),
                    block=%block.height,
                    signed_block=%signed_block_height,
                    "signature collection reward"
                );
                increase_value_for_key_and_era(
                    signing_validator,
                    signed_block_era,
                    contribution_reward,
                )?;
                increase_value_for_key_and_era(
                    block.proposer.clone(),
                    current_era_id,
                    collection_reward,
                )?;
            }
        }
    }

    let rewards_map_to_vec = |rewards_map: BTreeMap<EraId, Ratio<U512>>| {
        let min_era = rewards_map
            .iter()
            .find(|(_era, &amount)| !amount.numer().is_zero())
            .map(|(era, _amount)| era)
            .copied()
            .unwrap_or(current_era_id);
        EraId::iter_range_inclusive(min_era, current_era_id)
            .rev()
            .map(|era_id| {
                rewards_map
                    .get(&era_id)
                    .copied()
                    .unwrap_or(ratio_u512_zero)
                    .to_integer()
            })
            .collect()
    };

    // Return the rewards as plain U512:
    Ok(full_reward_for_validators
        .into_iter()
        .map(|(key, amounts)| (key, rewards_map_to_vec(amounts)))
        .collect())
}

/// Query all the blocks from the given range with a batch mechanism.
async fn collect_past_blocks_batched<REv: From<StorageRequest>>(
    effect_builder: EffectBuilder<REv>,
    era_height_span: Range<u64>,
) -> Result<Vec<CitedBlock>, RewardsError> {
    const STEP: usize = 100;
    let only_from_available_block_range = false;

    let batches = {
        let range_end = era_height_span.end;

        era_height_span
            .step_by(STEP)
            .map(move |internal_start| internal_start..range_end.min(internal_start + STEP as u64))
    };

    stream::iter(batches)
        .then(|range| async move {
            stream::iter(
                effect_builder
                    .collect_past_blocks_with_metadata(
                        range.clone(),
                        only_from_available_block_range,
                    )
                    .await
                    .into_iter()
                    .zip(range)
                    .map(|(maybe_block_with_metadata, height)| {
                        maybe_block_with_metadata
                            .ok_or(RewardsError::FailedToFetchBlockWithHeight(height))
                            .map(|b| CitedBlock::from(b.block))
                    }),
            )
        })
        .flatten()
        .try_collect()
        .await
}

impl From<Block> for CitedBlock {
    fn from(block: Block) -> Self {
        Self {
            protocol_version: block.protocol_version(),
            era_id: block.era_id(),
            height: block.height(),
            proposer: block.proposer().clone(),
            rewarded_signatures: block.rewarded_signatures().clone(),
            state_root_hash: *block.state_root_hash(),
            is_switch_block: block.is_switch_block(),
            is_genesis: block.is_genesis(),
        }
    }
}
