use super::{
    internal,
    providers::{StorageProvider},
    DelegationsMap, DelegatorRewardPoolMap, RewardPerStakeMap, TallyMap, TotalDelegatorStakeMap,
    DELEGATIONS_MAP_KEY, DELEGATOR_REWARD_POOL_MAP, REWARD_PER_STAKE_MAP_KEY, TALLY_MAP_KEY,
    TOTAL_DELEGATOR_STAKE_MAP_KEY,
};
use crate::system_contract_errors::auction::{Error, Result};
use internal::{read_from, write_to};

pub trait DataProvider: StorageProvider
where
    Error: From<<Self as StorageProvider>::Error>,
{
    /// Error representation for data provider errors.
    type Error: From<Error>;

    fn get_delegations_map(&mut self) -> Result<DelegationsMap> {
        read_from(self, DELEGATIONS_MAP_KEY)
    }
    fn set_delegations_map(&mut self, delegations_map: DelegationsMap) -> Result<()> {
        write_to(self, DELEGATIONS_MAP_KEY, delegations_map)
    }
    fn get_tally_map(&mut self) -> Result<TallyMap> {
        read_from(self, TALLY_MAP_KEY)
    }
    fn set_tally_map(&mut self, tally_map: TallyMap) -> Result<()> {
        write_to(self, TALLY_MAP_KEY, tally_map)
    }
    fn get_reward_per_stake_map(&mut self) -> Result<RewardPerStakeMap> {
        read_from(self, REWARD_PER_STAKE_MAP_KEY)
    }
    fn set_reward_per_stake_map(&mut self, reward_per_stake_map: RewardPerStakeMap) -> Result<()> {
        write_to(self, REWARD_PER_STAKE_MAP_KEY, reward_per_stake_map)
    }
    fn get_total_delegator_stake_map(&mut self) -> Result<TotalDelegatorStakeMap> {
        read_from(self, TOTAL_DELEGATOR_STAKE_MAP_KEY)
    }
    fn set_total_delegator_stake_map(
        &mut self,
        total_delegator_stake_map: TotalDelegatorStakeMap,
    ) -> Result<()> {
        write_to(
            self,
            TOTAL_DELEGATOR_STAKE_MAP_KEY,
            total_delegator_stake_map,
        )
    }
    fn get_delegator_reward_pool_map(&mut self) -> Result<DelegatorRewardPoolMap> {
        read_from(self, DELEGATOR_REWARD_POOL_MAP)
    }
    fn set_delegator_reward_pool_map(
        &mut self,
        delegator_reward_pool_map: DelegatorRewardPoolMap,
    ) -> Result<()> {
        write_to(self, DELEGATOR_REWARD_POOL_MAP, delegator_reward_pool_map)
    }
}
