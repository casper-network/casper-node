use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::{Bid, DelegationRate},
    CLType, CLTyped, PublicKey, U512,
};

/// The seigniorage recipient details.
/// Legacy version required to deserialize old records.
#[derive(Default, PartialEq, Eq, Clone, Debug)]
pub struct SeigniorageRecipientV1 {
    /// Validator stake (not including delegators)
    stake: U512,
    /// Delegation rate of a seigniorage recipient.
    delegation_rate: DelegationRate,
    /// Delegators and their bids.
    delegator_stake: BTreeMap<PublicKey, U512>,
}

impl SeigniorageRecipientV1 {
    /// Creates a new SeigniorageRecipient
    pub fn new(
        stake: U512,
        delegation_rate: DelegationRate,
        delegator_stake: BTreeMap<PublicKey, U512>,
    ) -> Self {
        Self {
            stake,
            delegation_rate,
            delegator_stake,
        }
    }

    /// Returns stake of the provided recipient
    pub fn stake(&self) -> &U512 {
        &self.stake
    }

    /// Returns delegation rate of the provided recipient
    pub fn delegation_rate(&self) -> &DelegationRate {
        &self.delegation_rate
    }

    /// Returns delegators of the provided recipient and their stake
    pub fn delegator_stake(&self) -> &BTreeMap<PublicKey, U512> {
        &self.delegator_stake
    }

    /// Calculates total stake, including delegators' total stake
    pub fn total_stake(&self) -> Option<U512> {
        self.delegator_total_stake()?.checked_add(self.stake)
    }

    /// Calculates total stake for all delegators
    pub fn delegator_total_stake(&self) -> Option<U512> {
        let mut total_stake: U512 = U512::zero();
        for stake in self.delegator_stake.values() {
            total_stake = total_stake.checked_add(*stake)?;
        }
        Some(total_stake)
    }
}

impl CLTyped for SeigniorageRecipientV1 {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for SeigniorageRecipientV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.stake.to_bytes()?);
        result.extend(self.delegation_rate.to_bytes()?);
        result.extend(self.delegator_stake.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.stake.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.delegator_stake.serialized_length()
    }
}

impl FromBytes for SeigniorageRecipientV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegator_stake, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            Self {
                stake,
                delegation_rate,
                delegator_stake,
            },
            bytes,
        ))
    }
}

impl From<&Bid> for SeigniorageRecipientV1 {
    fn from(bid: &Bid) -> Self {
        let delegator_stake = bid
            .delegators()
            .iter()
            .map(|(public_key, delegator)| (public_key.clone(), delegator.staked_amount()))
            .collect();
        Self {
            stake: *bid.staked_amount(),
            delegation_rate: *bid.delegation_rate(),
            delegator_stake,
        }
    }
}

/// The seigniorage recipient details with delegation rates for reservations.
#[derive(Default, PartialEq, Eq, Clone, Debug)]
pub struct SeigniorageRecipientV2 {
    /// Validator stake (not including delegators)
    stake: U512,
    /// Delegation rate of a seigniorage recipient.
    delegation_rate: DelegationRate,
    /// Delegators and their bids.
    delegator_stake: BTreeMap<PublicKey, U512>,
    /// Delegation rates for reserved slots
    reservation_delegation_rates: BTreeMap<PublicKey, DelegationRate>,
}

impl SeigniorageRecipientV2 {
    /// Creates a new SeigniorageRecipient
    pub fn new(
        stake: U512,
        delegation_rate: DelegationRate,
        delegator_stake: BTreeMap<PublicKey, U512>,
        reservation_delegation_rates: BTreeMap<PublicKey, DelegationRate>,
    ) -> Self {
        Self {
            stake,
            delegation_rate,
            delegator_stake,
            reservation_delegation_rates,
        }
    }

    /// Returns stake of the provided recipient
    pub fn stake(&self) -> &U512 {
        &self.stake
    }

    /// Returns delegation rate of the provided recipient
    pub fn delegation_rate(&self) -> &DelegationRate {
        &self.delegation_rate
    }

    /// Returns delegators of the provided recipient and their stake
    pub fn delegator_stake(&self) -> &BTreeMap<PublicKey, U512> {
        &self.delegator_stake
    }

    /// Calculates total stake, including delegators' total stake
    pub fn total_stake(&self) -> Option<U512> {
        self.delegator_total_stake()?.checked_add(self.stake)
    }

    /// Calculates total stake for all delegators
    pub fn delegator_total_stake(&self) -> Option<U512> {
        let mut total_stake: U512 = U512::zero();
        for stake in self.delegator_stake.values() {
            total_stake = total_stake.checked_add(*stake)?;
        }
        Some(total_stake)
    }

    /// Returns delegation rates for reservations of the provided recipient
    pub fn reservation_delegation_rates(&self) -> &BTreeMap<PublicKey, DelegationRate> {
        &self.reservation_delegation_rates
    }
}

impl CLTyped for SeigniorageRecipientV2 {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for SeigniorageRecipientV2 {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(self.stake.to_bytes()?);
        result.extend(self.delegation_rate.to_bytes()?);
        result.extend(self.delegator_stake.to_bytes()?);
        result.extend(self.reservation_delegation_rates.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.stake.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.delegator_stake.serialized_length()
            + self.reservation_delegation_rates.serialized_length()
    }
}

impl FromBytes for SeigniorageRecipientV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegator_stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (reservation_delegation_rates, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            Self {
                stake,
                delegation_rate,
                delegator_stake,
                reservation_delegation_rates,
            },
            bytes,
        ))
    }
}

impl From<&Bid> for SeigniorageRecipientV2 {
    fn from(bid: &Bid) -> Self {
        let delegator_stake = bid
            .delegators()
            .iter()
            .map(|(public_key, delegator)| (public_key.clone(), delegator.staked_amount()))
            .collect();
        Self {
            stake: *bid.staked_amount(),
            delegation_rate: *bid.delegation_rate(),
            delegator_stake,
            reservation_delegation_rates: BTreeMap::new(),
        }
    }
}

impl From<SeigniorageRecipientV1> for SeigniorageRecipientV2 {
    fn from(snapshot: SeigniorageRecipientV1) -> Self {
        Self {
            stake: snapshot.stake,
            delegation_rate: snapshot.delegation_rate,
            delegator_stake: snapshot.delegator_stake,
            reservation_delegation_rates: Default::default(),
        }
    }
}

/// Wrapper enum for all variants of `SeigniorageRecipient`.
#[allow(missing_docs)]
pub enum SeigniorageRecipient {
    V1(SeigniorageRecipientV1),
    V2(SeigniorageRecipientV2),
}

impl SeigniorageRecipient {
    /// Returns stake of the provided recipient
    pub fn stake(&self) -> &U512 {
        match self {
            Self::V1(recipient) => &recipient.stake,
            Self::V2(recipient) => &recipient.stake,
        }
    }

    /// Returns delegation rate of the provided recipient
    pub fn delegation_rate(&self) -> &DelegationRate {
        match self {
            Self::V1(recipient) => &recipient.delegation_rate,
            Self::V2(recipient) => &recipient.delegation_rate,
        }
    }

    /// Returns delegators of the provided recipient and their stake
    pub fn delegator_stake(&self) -> &BTreeMap<PublicKey, U512> {
        match self {
            Self::V1(recipient) => &recipient.delegator_stake,
            Self::V2(recipient) => &recipient.delegator_stake,
        }
    }

    /// Calculates total stake, including delegators' total stake
    pub fn total_stake(&self) -> Option<U512> {
        match self {
            Self::V1(recipient) => recipient.total_stake(),
            Self::V2(recipient) => recipient.total_stake(),
        }
    }

    /// Calculates total stake for all delegators
    pub fn delegator_total_stake(&self) -> Option<U512> {
        match self {
            Self::V1(recipient) => recipient.delegator_total_stake(),
            Self::V2(recipient) => recipient.delegator_total_stake(),
        }
    }

    /// Returns delegation rates for reservations of the provided recipient
    pub fn reservation_delegation_rates(&self) -> Option<&BTreeMap<PublicKey, DelegationRate>> {
        match self {
            Self::V1(_recipient) => None,
            Self::V2(recipient) => Some(&recipient.reservation_delegation_rates),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;
    use core::iter::FromIterator;

    use super::SeigniorageRecipientV2;
    use crate::{
        bytesrepr,
        system::auction::{DelegationRate, SeigniorageRecipientV1},
        PublicKey, SecretKey, U512,
    };

    #[test]
    fn serialization_roundtrip() {
        let delegator_1_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_2_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_3_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let seigniorage_recipient = SeigniorageRecipientV2 {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::MAX,
            delegator_stake: BTreeMap::from_iter(vec![
                (delegator_1_key.clone(), U512::max_value()),
                (delegator_2_key, U512::max_value()),
                (delegator_3_key, U512::zero()),
            ]),
            reservation_delegation_rates: BTreeMap::from_iter(vec![(
                delegator_1_key,
                DelegationRate::MIN,
            )]),
        };
        bytesrepr::test_serialization_roundtrip(&seigniorage_recipient);
    }

    #[test]
    fn serialization_roundtrip_legacy_version() {
        let delegator_1_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_2_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_3_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let legacy_seigniorage_recipient = SeigniorageRecipientV1 {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::MAX,
            delegator_stake: BTreeMap::from_iter(vec![
                (delegator_1_key.clone(), U512::max_value()),
                (delegator_2_key.clone(), U512::max_value()),
                (delegator_3_key.clone(), U512::zero()),
            ]),
        };

        bytesrepr::test_serialization_roundtrip(&legacy_seigniorage_recipient);
    }

    #[test]
    fn test_overflow_in_delegation_rate() {
        let delegator_1_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_2_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_3_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let seigniorage_recipient = SeigniorageRecipientV2 {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::MAX,
            delegator_stake: BTreeMap::from_iter(vec![
                (delegator_1_key.clone(), U512::max_value()),
                (delegator_2_key, U512::max_value()),
                (delegator_3_key, U512::zero()),
            ]),
            reservation_delegation_rates: BTreeMap::from_iter(vec![(
                delegator_1_key,
                DelegationRate::MIN,
            )]),
        };
        assert_eq!(seigniorage_recipient.total_stake(), None)
    }

    #[test]
    fn test_overflow_in_delegation_total_stake() {
        let delegator_1_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_2_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_3_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let seigniorage_recipient = SeigniorageRecipientV2 {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::max_value(),
            delegator_stake: BTreeMap::from_iter(vec![
                (delegator_1_key, U512::max_value()),
                (delegator_2_key, U512::max_value()),
                (delegator_3_key, U512::max_value()),
            ]),
            reservation_delegation_rates: BTreeMap::new(),
        };
        assert_eq!(seigniorage_recipient.delegator_total_stake(), None)
    }
}
