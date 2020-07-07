use alloc::{
    collections::{
        btree_map::{Iter, Values},
        BTreeMap,
    },
    format,
    string::String,
};

use crate::{
    account::AccountHash,
    system_contract_errors::pos::{Error, Result},
    U512,
};

/// The maximum difference between the largest and the smallest stakes.
// TODO: Should this be a percentage instead?
// TODO: Pick a reasonable value.
const MAX_SPREAD: U512 = U512::MAX;

/// The maximum increase of stakes in a single bonding request.
const MAX_INCREASE: U512 = U512::MAX;

/// The maximum decrease of stakes in a single unbonding request.
const MAX_DECREASE: U512 = U512::MAX;

/// The maximum increase of stakes in millionths of the total stakes in a single bonding request.
const MAX_REL_INCREASE: u64 = 1_000_000_000;

/// The maximum decrease of stakes in millionths of the total stakes in a single unbonding request.
const MAX_REL_DECREASE: u64 = 900_000;

/// The stakes map, assigning the staked amount of motes to each bonded
/// validator.
#[derive(Clone, Debug, PartialEq)]
pub struct Stakes(pub BTreeMap<AccountHash, U512>);

impl Stakes {
    /// Create new stakes map.
    pub fn new(map: BTreeMap<AccountHash, U512>) -> Stakes {
        Stakes(map)
    }

    /// Return an iterator to all items in stakes map.
    pub fn iter(&self) -> Iter<AccountHash, U512> {
        self.0.iter()
    }

    /// Return an iterator to values of a stakes map.
    pub fn values(&self) -> Values<AccountHash, U512> {
        self.0.values()
    }

    /// Return an iterator that encodes all entries as strings.
    pub fn strings(&self) -> impl Iterator<Item = String> + '_ {
        self.iter().map(|(account_hash, balance)| {
            let key_bytes = account_hash.as_bytes();
            let hex_key = base16::encode_lower(&key_bytes);
            format!("v_{}_{}", hex_key, balance)
        })
    }

    /// Returns total amount of bonds in a stakes map.
    pub fn total_bonds(&self) -> U512 {
        self.values().fold(U512::zero(), |x, y| x + y)
    }

    /// If `maybe_amount` is `None`, removes all the validator's stakes,
    /// otherwise subtracts the given amount. If the stakes are lower than
    /// the specified amount, it also subtracts all the stakes.
    ///
    /// Returns the amount that was actually subtracted from the stakes, or an
    /// error if
    /// * unbonding the specified amount is not allowed,
    /// * tries to unbond last validator,
    /// * validator was not bonded.
    pub fn unbond(&mut self, validator: &AccountHash, maybe_amount: Option<U512>) -> Result<U512> {
        let min = self
            .max_without(validator)
            .unwrap_or_else(U512::zero)
            .saturating_sub(MAX_SPREAD);
        let max_decrease = MAX_DECREASE.min(self.sum() * MAX_REL_DECREASE / 1_000_000);

        if let Some(amount) = maybe_amount {
            // The minimum stake value to not violate the maximum spread.
            let stake = self.0.get_mut(validator).ok_or(Error::NotBonded)?;
            if *stake > amount {
                if *stake - amount < min {
                    return Err(Error::SpreadTooHigh);
                }
                if amount > max_decrease {
                    return Err(Error::UnbondTooLarge);
                }
                *stake -= amount;
                return Ok(amount);
            }
        }
        if self.0.len() == 1 {
            return Err(Error::CannotUnbondLastValidator);
        }

        // If the the amount is greater or equal to the stake, remove the validator.
        let stake = self.0.remove(validator).ok_or(Error::NotBonded)?;

        if let Some(amount) = maybe_amount {
            if amount > stake {
                return Err(Error::UnbondTooLarge);
            }
        }

        if stake > min.saturating_add(max_decrease) && stake > max_decrease {
            return Err(Error::UnbondTooLarge);
        }
        Ok(stake)
    }

    /// Adds `amount` to the validator's stakes.
    pub fn bond(&mut self, validator: &AccountHash, amount: U512) {
        self.0
            .entry(*validator)
            .and_modify(|x| *x += amount)
            .or_insert(amount);
    }

    /// Returns an error if bonding the specified amount is not allowed.
    pub fn validate_bonding(&self, validator: &AccountHash, amount: U512) -> Result<()> {
        let max = self
            .min_without(validator)
            .unwrap_or(U512::MAX)
            .saturating_add(MAX_SPREAD);
        let min = self
            .max_without(validator)
            .unwrap_or_else(U512::zero)
            .saturating_sub(MAX_SPREAD);
        let stake = self.0.get(validator).map(|s| *s + amount).unwrap_or(amount);
        if stake > max || stake < min {
            return Err(Error::SpreadTooHigh);
        }
        let max_increase = MAX_INCREASE.min(self.sum() * MAX_REL_INCREASE / 1_000_000);
        if (stake.is_zero() && amount > min.saturating_add(max_increase))
            || (!stake.is_zero() && amount > max_increase)
        {
            return Err(Error::BondTooLarge);
        }
        Ok(())
    }

    /// Returns the minimum stake of the _other_ validators.
    fn min_without(&self, validator: &AccountHash) -> Option<U512> {
        self.0
            .iter()
            .filter(|(v, _)| *v != validator)
            .map(|(_, s)| s)
            .min()
            .cloned()
    }

    /// Returns the maximum stake of the _other_ validators.
    fn max_without(&self, validator: &AccountHash) -> Option<U512> {
        self.0
            .iter()
            .filter(|(v, _)| *v != validator)
            .map(|(_, s)| s)
            .max()
            .cloned()
    }

    /// Returns the total stakes.
    fn sum(&self) -> U512 {
        self.0
            .values()
            .fold(U512::zero(), |sum, s| sum.saturating_add(*s))
    }
}

#[cfg(test)]
mod tests {
    use crate::{account::AccountHash, system_contract_errors::pos::Error, U512};

    use super::Stakes;

    const KEY1: [u8; 32] = [1; 32];
    const KEY2: [u8; 32] = [2; 32];

    fn new_stakes(stakes: &[([u8; 32], u64)]) -> Stakes {
        Stakes(
            stakes
                .iter()
                .map(|&(key, amount)| (AccountHash::new(key), U512::from(amount)))
                .collect(),
        )
    }

    #[test]
    fn test_bond() {
        let mut stakes = new_stakes(&[(KEY2, 100)]);
        assert_eq!(
            Ok(()),
            stakes.validate_bonding(&AccountHash::new(KEY1), U512::from(5))
        );
        stakes.bond(&AccountHash::new(KEY1), U512::from(5));
        assert_eq!(new_stakes(&[(KEY1, 5), (KEY2, 100)]), stakes);
    }

    #[test]
    fn test_bond_existing() {
        let mut stakes = new_stakes(&[(KEY1, 50), (KEY2, 100)]);
        assert_eq!(
            Ok(()),
            stakes.validate_bonding(&AccountHash::new(KEY1), U512::from(4))
        );
        stakes.bond(&AccountHash::new(KEY1), U512::from(4));
        assert_eq!(new_stakes(&[(KEY1, 54), (KEY2, 100)]), stakes);
    }

    #[test]
    fn test_bond_too_much_rel() {
        let stakes = new_stakes(&[(KEY1, 1_000), (KEY2, 1_000)]);
        let total = 1_000 + 1_000;
        assert_eq!(
            Err(Error::BondTooLarge),
            stakes.validate_bonding(
                &AccountHash::new(KEY1),
                U512::from(super::MAX_REL_INCREASE * total / 1_000_000 + 1),
            ),
            "Successfully bonded more than the maximum amount."
        );
        assert_eq!(
            Ok(()),
            stakes.validate_bonding(
                &AccountHash::new(KEY1),
                U512::from(super::MAX_REL_INCREASE * total / 1_000_000),
            ),
            "Failed to bond the maximum amount."
        );
    }

    #[test]
    fn test_unbond() {
        let mut stakes = new_stakes(&[(KEY1, 5), (KEY2, 100)]);
        assert_eq!(
            Ok(U512::from(5)),
            stakes.unbond(&AccountHash::new(KEY1), None)
        );
        assert_eq!(new_stakes(&[(KEY2, 100)]), stakes);
    }

    #[test]
    fn test_unbond_last_validator() {
        let mut stakes = new_stakes(&[(KEY1, 5)]);
        assert_eq!(
            Err(Error::CannotUnbondLastValidator),
            stakes.unbond(&AccountHash::new(KEY1), None)
        );
    }

    #[test]
    fn test_partially_unbond() {
        let mut stakes = new_stakes(&[(KEY1, 50)]);
        assert_eq!(
            Ok(U512::from(4)),
            stakes.unbond(&AccountHash::new(KEY1), Some(U512::from(4)))
        );
        assert_eq!(new_stakes(&[(KEY1, 46)]), stakes);
    }

    #[test]
    fn test_unbond_too_much_rel() {
        let mut stakes = new_stakes(&[(KEY1, 999), (KEY2, 1)]);
        let total = 999 + 1;
        assert_eq!(
            Err(Error::UnbondTooLarge),
            stakes.unbond(
                &AccountHash::new(KEY1),
                Some(U512::from(super::MAX_REL_DECREASE * total / 1_000_000 + 1)),
            ),
            "Successfully unbonded more than the maximum amount."
        );
        assert_eq!(
            Ok(U512::from(super::MAX_REL_DECREASE * total / 1_000_000)),
            stakes.unbond(
                &AccountHash::new(KEY1),
                Some(U512::from(super::MAX_REL_DECREASE * total / 1_000_000)),
            ),
            "Failed to unbond the maximum amount."
        );
    }
}
