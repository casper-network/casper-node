use alloc::collections::BTreeMap;

use super::{
    data_provider::DataProvider,
    era_validators::ValidatorWeights,
    internal,
    providers::{MintProvider, RuntimeProvider, StorageProvider, SystemProvider},
    EraValidators, SeigniorageRecipient,
};
use crate::{
    system_contract_errors::auction::{Error, Result},
    URef, U512, PublicKey,
};

/// Update reward per stake according to pull-based distribution formulation
/// The entry for `validator_public_key` should be initialized by now
fn update_reward_per_stake(
    reward_per_stake_map: &mut BTreeMap<PublicKey, U512>,
    validator_public_key: PublicKey,
    amount: U512,
    total_delegator_stake: U512,
) {
    reward_per_stake_map
        .entry(validator_public_key)
        .and_modify(|rps| *rps += amount / total_delegator_stake);
}

/// Update delegations_map entry. Initialize if it doesn't exist.
fn update_delegations(
    delegations_map: &mut BTreeMap<PublicKey, BTreeMap<PublicKey, U512>>,
    validator_public_key: PublicKey,
    delegator_public_key: PublicKey,
    delegation_amount: U512,
) -> U512 {
    let new_quantity = *delegations_map
        .entry(validator_public_key)
        .or_default()
        .entry(delegator_public_key)
        .and_modify(|delegation| *delegation += delegation_amount)
        .or_insert_with(|| delegation_amount);
    new_quantity
}

/// Update total_delegator_stake entry. Initialize if it doesn't exist.
fn update_total_delegator_stake(
    total_delegator_stake_map: &mut BTreeMap<PublicKey, U512>,
    validator_public_key: PublicKey,
    delegation_amount: U512,
) {
    total_delegator_stake_map
        .entry(validator_public_key)
        .and_modify(|total_stake| *total_stake += delegation_amount)
        .or_insert_with(|| delegation_amount);
}

/// Update tally_map entry. Initialize if it doesn't exist.
fn update_tally(
    tally_map: &mut BTreeMap<PublicKey, BTreeMap<PublicKey, U512>>,
    validator_public_key: PublicKey,
    delegator_public_key: PublicKey,
    delegation_amount: U512,
    reward_per_stake: U512,
) {
    tally_map
        .entry(validator_public_key)
        .or_default()
        .entry(delegator_public_key)
        .and_modify(|tally| *tally += reward_per_stake * delegation_amount)
        .or_insert_with(|| reward_per_stake * delegation_amount);
}

/// Bonding auctions contract implementation.
pub trait DelegationProvider:
    DataProvider + StorageProvider + MintProvider + SystemProvider
where
    Error: From<<Self as DataProvider>::Error>
        + From<<Self as StorageProvider>::Error>
        + From<<Self as MintProvider>::Error>
        + From<<Self as SystemProvider>::Error>,
{
    /// Error representation for delegation provider errors.
    type Error: From<Error>;

    /// Adds a new delegator to delegators, or tops off a current
    /// one. If the target validator is not in active_bids, the function call
    /// returns a designated “failure” purse and does nothing. The function
    /// calls bond in the Mint contract to transfer motes to the
    /// validator’s purse and returns a tuple of that purse and the
    /// quantity of motes contained in it after the transfer.
    ///
    /// The arguments are the delegator’s key, the originating purse, the validator key and quantity of motes.
    fn delegate(
        &mut self,
        delegator_public_key: PublicKey,
        source_purse: URef,
        validator_public_key: PublicKey,
        delegation_amount: U512,
    ) -> Result<(URef, U512)> {
        let active_bids = internal::get_active_bids(self)?;
        // Return early if target validator is not in `active_bids`
        let _active_bid = active_bids
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let (bonding_purse, _total_amount) = self.bond(delegator_public_key, delegation_amount, source_purse)?;

        let mut delegations_map = self.get_delegations_map()?;
        let mut tally_map = self.get_tally_map()?;
        let mut total_delegator_stake_map = self.get_total_delegator_stake_map()?;
        let mut reward_per_stake_map = self.get_reward_per_stake_map()?;
        let mut delegator_reward_pool_map = self.get_delegator_reward_pool_map()?;

        // Get reward_per_stake_map entry. Initialize if it doesn't exist.
        let reward_per_stake = *reward_per_stake_map
            .entry(validator_public_key)
            .or_insert_with(|| U512::zero());

        update_total_delegator_stake(
            &mut total_delegator_stake_map,
            validator_public_key,
            delegation_amount,
        );
        // Initialize delegator_reward_pool_map entry if it doesn't exist.
        delegator_reward_pool_map
            .entry(validator_public_key)
            .or_default();

        let new_delegation_amount = update_delegations(
            &mut delegations_map,
            validator_public_key,
            delegator_public_key,
            delegation_amount,
        );
        update_tally(
            &mut tally_map,
            validator_public_key,
            delegator_public_key,
            delegation_amount,
            reward_per_stake,
        );

        self.set_delegations_map(delegations_map)?;
        self.set_total_delegator_stake_map(total_delegator_stake_map)?;
        self.set_tally_map(tally_map)?;
        self.set_reward_per_stake_map(reward_per_stake_map)?;
        self.set_delegator_reward_pool_map(delegator_reward_pool_map)?;

        Ok((bonding_purse, new_delegation_amount))
    }

    /// Removes a quantity (or the entry altogether, if the
    /// remaining quantity is 0) of motes from the entry in delegators
    /// and calls unbond in the Mint contract to create a new unbonding
    /// purse. Returns the new unbonding purse and the quantity of
    /// remaining delegated motes.
    ///
    /// The arguments are the delegator’s key, the validator key and quantity of motes.
    fn undelegate(
        &mut self,
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        quantity: U512,
    ) -> Result<U512> {
        let active_bids = internal::get_active_bids(self)?;

        let (_unbonding_purse, _total_amount) = self.unbond(delegator_public_key, quantity)?;

        // Return early if target validator is not in `active_bids`
        let _active_bid = active_bids
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let mut delegations_map = self.get_delegations_map()?;
        let mut tally_map = self.get_tally_map()?;
        let mut total_delegator_stake_map = self.get_total_delegator_stake_map()?;
        let mut reward_per_stake_map = self.get_reward_per_stake_map()?;

        let reward_per_stake = *reward_per_stake_map
            .get_mut(&validator_public_key)
            .unwrap();

        let delegations = delegations_map
            .get_mut(&validator_public_key)
            .ok_or(Error::DelegatorNotFound)?;

        // We don't have to check for existence in other maps, because the entries
        // are initialized at the same time in `delegate()`

        let new_amount = {
            let amount = delegations
                .get_mut(&delegator_public_key)
                .ok_or(Error::ValidatorNotFound)?;

            let new_amount = amount.checked_sub(quantity).ok_or(Error::InvalidQuantity)?;

            *amount = new_amount;
            new_amount
        };

        if new_amount.is_zero() {
            // Inner map's mapped value should be zero as we subtracted mutable value.
            let _value = delegations.remove(&delegator_public_key).unwrap();
            debug_assert!(_value.is_zero());
        }

        // Update total_delegator_stake_map entry
        let new_total_amount = {
            let total_amount = total_delegator_stake_map
                .get_mut(&validator_public_key)
                .ok_or(Error::ValidatorNotFound)?;

            let new_total_amount = total_amount
                .checked_sub(quantity)
                .ok_or(Error::InvalidQuantity)?;

            *total_amount = new_total_amount;
            new_total_amount
        };

        if new_total_amount.is_zero() {
            // Inner map's mapped value should be zero as we subtracted mutable value.
            let _value = total_delegator_stake_map
                .remove(&validator_public_key)
                .unwrap();
            debug_assert!(_value.is_zero());
        }

        // Update tally_map entry
        let _new_tally_amount = {
            let tally_amount = tally_map
                .get_mut(&validator_public_key)
                .unwrap()
                .get_mut(&delegator_public_key)
                .ok_or(Error::DelegatorNotFound)?;

            let new_tally_amount = tally_amount
                .checked_sub(reward_per_stake * quantity)
                .ok_or(Error::InvalidQuantity)?;

            *tally_amount = new_tally_amount;
            new_tally_amount
        };

        // TODO: Reconsider check after reviewing formulation
        // if new_tally_amount.is_zero() {
        //     // Inner map's mapped value should be zero as we subtracted mutable value.
        //     let _value = tally_map.remove(&validator_public_key).unwrap();
        //     debug_assert!(_value.is_zero());
        // }

        self.set_delegations_map(delegations_map)?;
        self.set_total_delegator_stake_map(total_delegator_stake_map)?;
        self.set_tally_map(tally_map)?;

        Ok(new_amount)
    }

    /// Distributes rewards to the delegators associated with `validator_public_key`.
    /// Uses pull-based reward distribution formulation to distribute seigniorage
    /// rewards to all delegators of a validator, proportional to delegation amounts
    /// with O(1) time complexity.
    ///
    /// Accessed by: PoS contract
    fn distribute_to_delegators(
        &mut self,
        validator_public_key: PublicKey,
        purse: URef,
    ) -> Result<()> {
        let total_delegator_stake = *self
            .get_total_delegator_stake_map()?
            .get(&validator_public_key)
            .unwrap();

        let amount = self.get_balance(purse)?.unwrap_or_default();

        // Throw an error if the validator has no delegations
        if total_delegator_stake.eq(&U512::zero()) {
            return Err(Error::MissingDelegations);
        }

        let mut reward_per_stake_map = self.get_reward_per_stake_map()?;
        update_reward_per_stake(
            &mut reward_per_stake_map,
            validator_public_key,
            amount,
            total_delegator_stake,
        );
        self.set_reward_per_stake_map(reward_per_stake_map)?;

        // Get the reward pool purse for the validator
        let delegator_reward_pool = *self
            .get_delegator_reward_pool_map()?
            .get(&validator_public_key)
            .unwrap();

        // Transfer the reward to the reward pool purse
        self.transfer_from_purse_to_purse(purse, delegator_reward_pool, amount)?;

        Ok(())
    }

    /// Returns the total rewards a delegator has earned from delegating to a specific validator.
    /// Read-only.
    fn delegation_reward(
        &mut self,
        validator_public_key: PublicKey,
        delegator_public_key: PublicKey,
    ) -> Result<U512> {
        let delegation = *self
            .get_delegations_map()?
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?
            .get(&delegator_public_key)
            .ok_or(Error::DelegatorNotFound)?;

        let tally = *self
            .get_tally_map()?
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?
            .get(&delegator_public_key)
            .ok_or(Error::DelegatorNotFound)?;

        let reward_per_stake = *self
            .get_reward_per_stake_map()?
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let reward = delegation * reward_per_stake - tally;
        Ok(reward)
    }

    /// Allows delegators to withdraw the seigniorage rewards they have earned.
    /// Pays out the entire accumulated amount to the destination purse.
    fn withdraw_reward(
        &mut self,
        validator_public_key: PublicKey,
        delegator_public_key: PublicKey,
        purse: URef,
    ) -> Result<U512> {
        // Get the amount of reward to be paid out
        let reward = self.delegation_reward(validator_public_key, delegator_public_key)?;

        // Get the required variables
        let reward_pool = *self
            .get_delegator_reward_pool_map()?
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        let delegation = *self
            .get_delegations_map()?
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?
            .get(&delegator_public_key)
            .ok_or(Error::DelegatorNotFound)?;

        let reward_per_stake = *self
            .get_reward_per_stake_map()?
            .get(&validator_public_key)
            .ok_or(Error::ValidatorNotFound)?;

        // Reset tally for the delegator
        let mut tally_map = self.get_tally_map()?;
        tally_map
            .entry(validator_public_key)
            .or_default()
            .entry(delegator_public_key)
            .and_modify(|tally| *tally = reward_per_stake * delegation);

        // Finally, transfer rewards to the delegator
        self.transfer_from_purse_to_purse(reward_pool, purse, reward)?;

        Ok(reward)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auction::{
        delegator::{DelegatorRewardPoolMap, RewardPerStakeMap, TallyMap, TotalDelegatorStakeMap},
        DelegationsMap,
    };

    struct PullBasedDistMockup {
        delegations_map: DelegationsMap,
        reward_per_stake_map: RewardPerStakeMap,
        total_delegator_stake_map: TotalDelegatorStakeMap,
        tally_map: TallyMap,
        delegator_reward_pool_map: DelegatorRewardPoolMap,
    }

    impl PullBasedDistMockup {
        /// Create a toy stake & delegation distribution
        fn new() -> Self {
            Self {
                delegations_map: DelegationsMap::default(),
                reward_per_stake_map: RewardPerStakeMap::default(),
                total_delegator_stake_map: TotalDelegatorStakeMap::default(),
                tally_map: TallyMap::default(),
                delegator_reward_pool_map: DelegatorRewardPoolMap::default(),
            }
        }
    }

    fn prepare_mockup(mockup: &mut PullBasedDistMockup) {
        let validator_public_key = PublicKey::Ed25519([1; 32]);
        let delegator1_account_hash = PublicKey::Ed25519([2; 32]);
        let delegator2_account_hash = PublicKey::Ed25519([3; 32]);
        let delegator3_account_hash = PublicKey::Ed25519([4; 32]);

        let total_reward_amount = U512::from(10_000_000_000u64);
        let total_delegator_stake = U512::from(30_000_000_000_000u64);
        let initial_reward_per_stake = U512::from(90_000_000_000u64);
        let mut reward_per_stake_map = mockup.get_reward_per_stake_map().unwrap();

        reward_per_stake_map.insert(validator_public_key, initial_reward_per_stake);

        mockup.set_reward_per_stake_map(reward_per_stake_map);
    }

    impl DelegationProvider for PullBasedDistMockup {
        type Error = Error;
    }

    impl MintProvider for PullBasedDistMockup {
        type Error = Error;
        fn bond(
        &mut self,
        public_key: PublicKey,
        amount: U512,
        purse: URef,
    ) -> Result<(URef, U512)> {
        todo!()
    }
        fn unbond(&mut self, public_key: PublicKey, amount: U512) -> Result<(URef, U512)> {
        todo!()
    }
        
    }

    impl SystemProvider for PullBasedDistMockup {
        type Error = Error;
        fn create_purse(&mut self) -> URef {
            todo!()
        }
        fn get_balance(&mut self, purse: URef) -> Result<Option<U512>,> {
            todo!()
        }
        fn transfer_from_purse_to_purse(
            &mut self,
            source: URef,
            target: URef,
            amount: U512,
        ) -> Result<()> {
            todo!()
        }
    }

    impl DataProvider for PullBasedDistMockup {
        type Error = Error;
        fn get_delegations_map(&mut self) -> Result<DelegationsMap> {
            Ok(self.delegations_map.clone())
        }
        fn set_delegations_map(&mut self, delegations_map: DelegationsMap) -> Result<()> {
            self.delegations_map = delegations_map;
            Ok(())
        }
        fn get_tally_map(&mut self) -> Result<TallyMap> {
            Ok(self.tally_map.clone())
        }
        fn set_tally_map(&mut self, tally_map: TallyMap) -> Result<()> {
            self.tally_map = tally_map;
            Ok(())
        }
        fn get_reward_per_stake_map(&mut self) -> Result<RewardPerStakeMap> {
            Ok(self.reward_per_stake_map.clone())
        }
        fn set_reward_per_stake_map(
            &mut self,
            reward_per_stake_map: RewardPerStakeMap,
        ) -> Result<()> {
            self.reward_per_stake_map = reward_per_stake_map;
            Ok(())
        }
        fn get_total_delegator_stake_map(&mut self) -> Result<TotalDelegatorStakeMap> {
            Ok(self.total_delegator_stake_map.clone())
        }
        fn set_total_delegator_stake_map(
            &mut self,
            total_delegator_stake_map: TotalDelegatorStakeMap,
        ) -> Result<()> {
            self.total_delegator_stake_map = total_delegator_stake_map;
            Ok(())
        }
        fn get_delegator_reward_pool_map(&mut self) -> Result<DelegatorRewardPoolMap> {
            Ok(self.delegator_reward_pool_map.clone())
        }
        fn set_delegator_reward_pool_map(
            &mut self,
            delegator_reward_pool_map: DelegatorRewardPoolMap,
        ) -> Result<()> {
            self.delegator_reward_pool_map = delegator_reward_pool_map;
            Ok(())
        }
    }

    impl StorageProvider for PullBasedDistMockup {
        type Error = Error;
        fn get_key(&mut self, _name: &str) -> Option<crate::Key> {
            todo!()
        }
        fn read<T: crate::bytesrepr::FromBytes + crate::CLTyped>(
            &mut self,
            _uref: URef,
        ) -> Result<Option<T>> {
            todo!()
        }
        fn write<T: crate::bytesrepr::ToBytes + crate::CLTyped>(
            &mut self,
            _uref: URef,
            _value: T,
        ) -> Result<()> {
            todo!()
        }
    }

    #[test]
    fn test_update_reward_per_stake() {
        let mut reward_per_stake_map = BTreeMap::new();
        let total_reward_amount = U512::from(10_000_000_000u64);
        let total_delegator_stake = U512::from(30_000_000_000_000u64);
        let validator_public_key = PublicKey::Ed25519([50; 32]);
        let initial_reward_per_stake = U512::from(90_000_000_000u64);
        reward_per_stake_map.insert(validator_public_key, initial_reward_per_stake);

        update_reward_per_stake(
            &mut reward_per_stake_map,
            validator_public_key,
            total_reward_amount,
            total_delegator_stake,
        );

        assert!(
            *reward_per_stake_map.get(&validator_public_key).unwrap()
                == initial_reward_per_stake + total_reward_amount / total_delegator_stake
        );
    }

    #[test]
    fn test_distribute_and_withdraw() {
        let mut mockup = PullBasedDistMockup::new();
        prepare_mockup(&mut mockup);
    }
}
