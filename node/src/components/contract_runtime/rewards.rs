use std::{collections::BTreeMap, ops::Range, sync::Arc};

use casper_execution_engine::engine_state::{self, GetEraValidatorsError};
use futures::stream::{self, StreamExt as _, TryStreamExt as _};
use num_rational::Ratio;
use num_traits::{CheckedAdd, CheckedMul};

use crate::{
    contract_runtime::EraValidatorsRequest,
    effect::{
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder,
    },
};
use casper_types::{Block, BlockHash, Chainspec, Digest, EraId, ProtocolVersion, PublicKey, U512};

pub(crate) trait ReactorEventT:
    Send + From<StorageRequest> + From<ContractRuntimeRequest>
{
}

impl<T> ReactorEventT for T where T: Send + From<StorageRequest> + From<ContractRuntimeRequest> {}

struct ErasInfo(BTreeMap<EraId, EraInfo>);

/// The era information needed in the rewards computation:
#[derive(Clone)]
struct EraInfo {
    weights: BTreeMap<PublicKey, U512>,
    total_weights: U512,
    reward_per_round: Ratio<U512>,
}

#[derive(Debug)]
pub enum RewardsError {
    /// There were an error while trying to get the eras information.
    PopulateError(PopulateError),
    /// We got a block height which is not in the era range it should be in (should not happen).
    HeightNotInEraRange(u64),
    /// The era is not in the range we have (should not happen).
    EraIdNotInEraRange(EraId),
    /// The validator public key is not in the era it should be in (should not happen).
    ValidatorKeyNotInEra(Box<PublicKey>),
    /// We didn't have a required switch block.
    MissingSwitchBlock(EraId),
    /// We got an overflow while computing something.
    ArithmeticOverflow,
}

/// We could not fetch the asked information for the given eras.
#[derive(Debug)]
pub enum PopulateError {
    FailedToFetchBlock(BlockHash),
    FailedToFetchEra(GetEraValidatorsError),
    /// Fetching the era validators succedeed, but no info is present (should not happen).
    /// The `Digest` is the one that was queried.
    NoEraReturned(Digest),
    FailedToFetchTotalSupply(engine_state::Error),
    FailedToFetchSeigniorageRate(engine_state::Error),
}

impl ErasInfo {
    /// `block_hashs` is an iterator over the era ID to get the information about + the block
    /// hash to query to have such information (which may not be from the same era).
    pub async fn populate<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        protocol_version: ProtocolVersion,
        block_hashes: impl IntoIterator<Item = (EraId, BlockHash)>,
    ) -> Result<Self, PopulateError> {
        let eras_info = stream::iter(block_hashes)
            .then(|(era_id, block_hash)| async move {
                let state_root_hash = effect_builder
                    .get_block_from_storage(block_hash)
                    .await
                    .map(|block| *block.state_root_hash())
                    .ok_or_else(|| PopulateError::FailedToFetchBlock(block_hash))?;
                let weights = effect_builder
                    .get_era_validators_from_contract_runtime(EraValidatorsRequest::new(
                        state_root_hash,
                        protocol_version,
                    ))
                    .await
                    .map_err(PopulateError::FailedToFetchEra)?
                    // We consume the map to not clone the value:
                    .into_iter()
                    .find(|(key, _)| key == &era_id)
                    .ok_or_else(|| PopulateError::NoEraReturned(state_root_hash))?
                    .1;
                let total_supply = effect_builder
                    .get_total_supply(state_root_hash)
                    .await
                    .map_err(PopulateError::FailedToFetchTotalSupply)?;
                let seignorate_rate = effect_builder
                    .get_round_seigniorage_rate(state_root_hash)
                    .await
                    .map_err(PopulateError::FailedToFetchSeigniorageRate)?;
                let reward_per_round = seignorate_rate * total_supply;
                let total_weights = weights.values().copied().sum();

                Ok((
                    era_id,
                    EraInfo {
                        weights,
                        total_weights,
                        reward_per_round,
                    },
                ))
            })
            .try_collect()
            .await?;
        let eras_info = ErasInfo(eras_info);

        Ok(eras_info)
    }

    /// We cannot get the genesis info from a root hash, so we copy it from era 1 when needed.
    pub fn copy_genesis_info_from_era_1(&mut self) -> Result<(), RewardsError> {
        let era_1 = EraId::from(1);
        let era_1_info = self
            .0
            .get(&era_1)
            .ok_or(RewardsError::EraIdNotInEraRange(era_1))?;
        self.0.insert(EraId::from(0), era_1_info.clone());

        Ok(())
    }

    /// Returns the validators from a given era.
    pub fn validator_keys(
        &self,
        era_id: EraId,
    ) -> Result<impl Iterator<Item = PublicKey> + '_, RewardsError> {
        let keys = self
            .0
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
            .0
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
            .0
            .get(&era_id)
            .ok_or(RewardsError::EraIdNotInEraRange(era_id))?;
        let weight = era
            .weights
            .get(validator)
            .ok_or_else(|| RewardsError::ValidatorKeyNotInEra(Box::new(validator.clone())))?;

        Ok(Ratio::new(*weight, era.total_weights))
    }

    /// For debugging purpose.
    fn era_ids(&self) -> Vec<u64> {
        self.0.keys().map(|id| id.value()).collect()
    }
}

pub(crate) async fn rewards_for_era<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    era_id: EraId,
    switch_block_height: u64,
    chainspec: Arc<Chainspec>,
) -> Result<BTreeMap<PublicKey, U512>, RewardsError> {
    fn increase_value_for_key(
        map: &mut BTreeMap<PublicKey, Ratio<U512>>,
        key: PublicKey,
        value: Ratio<U512>,
    ) -> Result<(), RewardsError> {
        match map.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(value);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let new_value = (*entry.get())
                    .checked_add(&value)
                    .ok_or(RewardsError::ArithmeticOverflow)?;
                *entry.get_mut() = new_value;
            }
        }

        Ok(())
    }

    fn to_ratio_u512(ratio: Ratio<u64>) -> Ratio<U512> {
        Ratio::new(U512::from(*ratio.numer()), U512::from(*ratio.denom()))
    }

    // Special case: genesis block does not yield any reward, because there is no block producer,
    // and no previous blocks whose signatures are to be rewarded:
    if era_id.is_genesis() {
        return Ok(BTreeMap::new());
    }

    // All the blocks that may appear as a signed block. They are collected upfront, so that we
    // don't have to worry about doing it one by one later.
    //
    // They are sorted from the oldest to the newest:
    let cited_blocks = {
        let cited_block_height_start = {
            let previous_era_id = era_id.saturating_sub(1);
            let previous_era_switch_block_header = effect_builder
                .get_switch_block_header_by_era_id_from_storage(previous_era_id)
                .await
                .ok_or(RewardsError::MissingSwitchBlock(previous_era_id))?;

            // Here we do not substract 1, because we want one block more:
            previous_era_switch_block_header
                .height()
                .saturating_sub(chainspec.core_config.signature_rewards_max_delay)
        };
        let range_to_fetch = cited_block_height_start..switch_block_height;

        let result = collect_past_blocks_batched(effect_builder, range_to_fetch.clone()).await;

        tracing::info!(
            era_id = %era_id.value(),
            range_requested = ?range_to_fetch,
            num_fetched_blocks = %result.iter().flatten().count(),
            num_requested_blocks = %result.len(),
            "blocks fetched",
        );

        result
    };

    let eras_info = {
        let mut cited_blocks = cited_blocks.iter().flatten();

        // We take the first block, then every switch block to get all the needed hashes to fetch
        // needed eras. We take the first block, because we need it for the first era.
        // If the first block is itself a switch block, that's fine, because we fetch one block more
        // to handle this case.

        let oldest_block = cited_blocks.next();

        // If the oldest block is genesis, we add the validator information for genesis (era 0) from
        // era 1, because it's the same:
        let oldest_block_is_genesis = oldest_block.map_or(false, |block| block.is_genesis());

        let hashes_and_eras: Vec<_> = oldest_block
            .into_iter()
            .chain(cited_blocks.filter(|&block| block.is_switch_block()))
            .map(|block| {
                (
                    // The validator info for a switch block is the one from the next era:
                    if block.is_switch_block() {
                        block.era_id().successor()
                    } else {
                        block.era_id()
                    },
                    *block.hash(),
                )
            })
            .collect();

        let num_eras_to_fetch = hashes_and_eras.len() + usize::from(oldest_block_is_genesis);

        let mut result = ErasInfo::populate(
            effect_builder,
            chainspec.protocol_version(),
            hashes_and_eras,
        )
        .await
        .map_err(RewardsError::PopulateError)?;

        if oldest_block_is_genesis {
            result.copy_genesis_info_from_era_1()?;
        }

        tracing::info!(
            era_id = %era_id.value(),
            %num_eras_to_fetch,
            eras_fetched = ?result.era_ids(),
        );

        result
    };

    let collection_proportion =
        to_ratio_u512(chainspec.core_config.collection_rewards_proportion());
    let contribution_proportion =
        to_ratio_u512(chainspec.core_config.contribution_rewards_proportion());

    // Reward for producing a block from this era:
    let production_reward = to_ratio_u512(chainspec.core_config.production_rewards_proportion())
        .checked_mul(&eras_info.reward(era_id)?)
        .ok_or(RewardsError::ArithmeticOverflow)?;

    // Collect all rewards as a ratio:
    let full_reward_for_validators = {
        let mut result = BTreeMap::new();

        let blocks_from_current_era = cited_blocks.iter().filter_map(|maybe_block| {
            maybe_block
                .as_ref()
                .filter(|block| block.era_id() == era_id)
        });

        for block in blocks_from_current_era {
            // Transfer the block production reward for this block proposer:
            increase_value_for_key(&mut result, block.proposer().clone(), production_reward)?;

            // Now, let's compute the reward attached to each signed block reported by the block
            // we examine:
            for (signature_rewards, signed_block_height) in block
                .rewarded_signatures()
                .iter()
                .zip((0..block.height()).rev())
            {
                let signed_block_era = cited_blocks
                    .iter()
                    .flatten()
                    .find_map(|block| {
                        (block.height() == signed_block_height).then_some(block.era_id())
                    })
                    .ok_or_else(|| RewardsError::HeightNotInEraRange(block.height()))?;
                let validators_providing_signature =
                    signature_rewards.to_validator_set(eras_info.validator_keys(signed_block_era)?);

                for signing_validator in validators_providing_signature {
                    // Reward for contributing to the finality signature, ie signing this block:
                    let contribution_reward = eras_info
                        .weight_ratio(signed_block_era, &signing_validator)?
                        .checked_mul(&contribution_proportion)
                        .ok_or(RewardsError::ArithmeticOverflow)?
                        .checked_mul(&eras_info.reward(signed_block_era)?)
                        .ok_or(RewardsError::ArithmeticOverflow)?;
                    // Reward for gathering this signature. It is both weighted by the block
                    // producing/signature collecting validator, and the signing validator:
                    let collection_reward = eras_info
                        .weight_ratio(signed_block_era, &signing_validator)?
                        .checked_mul(&collection_proportion)
                        .ok_or(RewardsError::ArithmeticOverflow)?
                        .checked_mul(&eras_info.reward(signed_block_era)?)
                        .ok_or(RewardsError::ArithmeticOverflow)?;

                    increase_value_for_key(&mut result, signing_validator, contribution_reward)?;
                    increase_value_for_key(
                        &mut result,
                        block.proposer().clone(),
                        collection_reward,
                    )?;
                }
            }
        }

        result
    };

    // Return the rewards as plain U512:
    Ok(full_reward_for_validators
        .into_iter()
        .map(|(key, amount)| (key, amount.to_integer()))
        .collect())
}

/// Query all the blocks from the given range with a batch mechanism.
async fn collect_past_blocks_batched<REv: From<StorageRequest>>(
    effect_builder: EffectBuilder<REv>,
    era_height_span: Range<u64>,
) -> Vec<Option<Block>> {
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
                    .collect_past_blocks_with_metadata(range, only_from_available_block_range)
                    .await
                    .into_iter()
                    .map(|maybe_block_with_metadata| maybe_block_with_metadata.map(|b| b.block)),
            )
        })
        .flatten()
        .collect()
        .await
}
