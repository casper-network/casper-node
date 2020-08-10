use alloc::{collections::BTreeMap, vec::Vec};

use super::{types::CommissionRate, Delegations, delegator::Tally, EraId};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, U512, URef, AccessRights
};

/// The seigniorage recipient details.
#[cfg_attr(test, derive(Debug))]
#[derive(Default, PartialEq, Clone)]
pub struct SeigniorageRecipient {
    /// Total staked amounts
    pub stake: U512,
    /// Accumulated delegation rates.
    pub commission_rate: CommissionRate,
    /// Sum of delegators' stake
    pub total_delegator_stake: U512,
    /// Helper variable tracking how much reward has been earned per staked token
    pub reward_per_stake: U512,
    /// Maps delegator accounts to delegated amounts
    pub delegations: Delegations,
    /// Map for internal accounting for pull-based distribution
    pub tally: Tally,
    /// Purse that holds tokens rewarded to delegators
    pub delegator_reward_pool: URef,
}

impl SeigniorageRecipient {
    pub fn new(
        stake: U512,
        commission_rate: CommissionRate,
        total_delegator_stake: U512,
        reward_per_stake: U512,
        delegations: BTreeMap<AccountHash, U512>,
        tally: BTreeMap<AccountHash, U512>,
        delegator_reward_purse: URef,
    ) -> Self {
        Self {
            stake,
            commission_rate,
            total_delegator_stake,
            reward_per_stake,
            delegations,
            tally,
            delegator_reward_pool: delegator_reward_purse,
        }
    }
}

impl CLTyped for SeigniorageRecipient {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for SeigniorageRecipient {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.stake.to_bytes()?);
        result.extend(self.commission_rate.to_bytes()?);
        result.extend(self.total_delegator_stake.to_bytes()?);
        result.extend(self.reward_per_stake.to_bytes()?);
        result.extend(self.delegations.to_bytes()?);
        result.extend(self.tally.to_bytes()?);
        result.extend(self.delegator_reward_pool.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.stake.serialized_length()
            + self.commission_rate.serialized_length()
            + self.total_delegator_stake.serialized_length()
            + self.reward_per_stake.serialized_length()
            + self.delegations.serialized_length()
            + self.tally.serialized_length()
            + self.delegator_reward_pool.serialized_length()
    }
}

impl FromBytes for SeigniorageRecipient {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (commission_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (total_delegator_stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (reward_per_stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegations, bytes) = FromBytes::from_bytes(bytes)?;
        let (tally, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegator_reward_purse, bytes) = FromBytes::from_bytes(bytes)?;

        Ok((
            SeigniorageRecipient {
                stake,
                commission_rate,
                total_delegator_stake,
                reward_per_stake,
                delegations,
                tally,
                delegator_reward_pool: delegator_reward_purse,
            },
            bytes,
        ))
    }
}

/// Collection of seigniorage recipients.
pub type SeigniorageRecipients = BTreeMap<PublicKey, SeigniorageRecipient>;

/// Snapshot of `SeigniorageRecipients` for a given era.
pub type SeigniorageRecipientsSnapshot = BTreeMap<EraId, SeigniorageRecipients>;

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;
    use core::iter::FromIterator;

    use super::SeigniorageRecipient;
    use crate::{account::AccountHash, auction::CommissionRate, bytesrepr, U512, PublicKey, URef, AccessRights};

    #[test]
    fn serialization_roundtrip() {
        let seigniorage_recipient = SeigniorageRecipient {
            stake: U512::max_value(),
            delegations: BTreeMap::from_iter(vec![
                (PublicKey::Ed25519([42; 32]), U512::one()),
                (PublicKey::Ed25519([43; 32]), U512::max_value()),
                (PublicKey::Ed25519([44; 32]), U512::zero()),
            ]),
            commission_rate: CommissionRate::max_value(),
            total_delegator_stake: U512::zero(),
            reward_per_stake: U512::zero(),
            tally: BTreeMap::from_iter(vec![
                (AccountHash::new([45; 32]), U512::one()),
                (AccountHash::new([46; 32]), U512::max_value()),
                (AccountHash::new([47; 32]), U512::zero()),
            ]),
            delegator_reward_pool: URef::new([48; 32], AccessRights::NONE),
        };
        bytesrepr::test_serialization_roundtrip(&seigniorage_recipient);
    }
}
