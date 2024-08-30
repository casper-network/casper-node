use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::auction::{Bid, DelegationRate},
    CLType, CLTyped, PublicKey, U512,
};

/// The seigniorage recipient details.
#[derive(Default, PartialEq, Eq, Clone, Debug)]
pub struct SeigniorageRecipient {
    /// Validator stake (not including delegators)
    stake: U512,
    /// Delegation rate of a seigniorage recipient.
    delegation_rate: DelegationRate,
    /// Delegators and their bids.
    delegator_stake: BTreeMap<PublicKey, U512>,
    /// Delegation rates for reserved slots
    reservation_delegation_rates: BTreeMap<PublicKey, DelegationRate>,
}

impl SeigniorageRecipient {
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

impl CLTyped for SeigniorageRecipient {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for SeigniorageRecipient {
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

impl FromBytes for SeigniorageRecipient {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (stake, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        let (delegator_stake, bytes) = FromBytes::from_bytes(bytes)?;
        // handle deserialization of legacy records without `reservation_delegation_rates` field
        let (reservation_delegation_rates, bytes) = if bytes.is_empty() {
            (BTreeMap::new(), bytes)
        } else {
            FromBytes::from_bytes(bytes)?
        };
        Ok((
            SeigniorageRecipient {
                stake,
                delegation_rate,
                delegator_stake,
                reservation_delegation_rates,
            },
            bytes,
        ))
    }
}

impl From<&Bid> for SeigniorageRecipient {
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

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;
    use core::iter::FromIterator;

    use crate::{
        bytesrepr,
        bytesrepr::{deserialize, deserialize_from_slice, ToBytes},
        system::auction::{DelegationRate, SeigniorageRecipient},
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
        let seigniorage_recipient = SeigniorageRecipient {
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
        struct LegacySeigniorageRecipient {
            /// Validator stake (not including delegators)
            stake: U512,
            /// Delegation rate of a seigniorage recipient.
            delegation_rate: DelegationRate,
            /// Delegators and their bids.
            delegator_stake: BTreeMap<PublicKey, U512>,
        }

        impl ToBytes for LegacySeigniorageRecipient {
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

        let delegator_1_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_2_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let delegator_3_key = PublicKey::from(
            &SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let legacy_seigniorage_recipient = LegacySeigniorageRecipient {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::MAX,
            delegator_stake: BTreeMap::from_iter(vec![
                (delegator_1_key.clone(), U512::max_value()),
                (delegator_2_key.clone(), U512::max_value()),
                (delegator_3_key.clone(), U512::zero()),
            ]),
        };

        let seigniorage_recipient = SeigniorageRecipient {
            stake: U512::max_value(),
            delegation_rate: DelegationRate::MAX,
            delegator_stake: BTreeMap::from_iter(vec![
                (delegator_1_key, U512::max_value()),
                (delegator_2_key, U512::max_value()),
                (delegator_3_key, U512::zero()),
            ]),
            reservation_delegation_rates: BTreeMap::new(),
        };

        let serialized = legacy_seigniorage_recipient
            .to_bytes()
            .expect("Unable to serialize data");
        assert_eq!(
            serialized.len(),
            legacy_seigniorage_recipient.serialized_length()
        );

        let mut written_bytes = vec![];
        legacy_seigniorage_recipient
            .write_bytes(&mut written_bytes)
            .expect("Unable to serialize data via write_bytes");
        assert_eq!(serialized, written_bytes);

        let deserialized_from_slice =
            deserialize_from_slice(&serialized).expect("Unable to deserialize data");
        assert_eq!(seigniorage_recipient, deserialized_from_slice);

        let deserialized =
            deserialize::<SeigniorageRecipient>(serialized).expect("Unable to deserialize data");
        assert_eq!(seigniorage_recipient, deserialized);
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
        let seigniorage_recipient = SeigniorageRecipient {
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
        let seigniorage_recipient = SeigniorageRecipient {
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
