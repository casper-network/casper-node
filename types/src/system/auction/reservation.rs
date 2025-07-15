use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey,
};

use super::{DelegationRate, DelegatorKind};

/// Represents a validator reserving a slot for specific delegator
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    /// Delegator kind.
    delegator_kind: DelegatorKind,
    /// Validator public key.
    validator_public_key: PublicKey,
    /// Individual delegation rate.
    delegation_rate: DelegationRate,
}

impl Reservation {
    /// Creates a new [`Reservation`]
    pub fn new(
        validator_public_key: PublicKey,
        delegator_kind: DelegatorKind,
        delegation_rate: DelegationRate,
    ) -> Self {
        Self {
            delegator_kind,
            validator_public_key,
            delegation_rate,
        }
    }

    /// Returns kind of delegator.
    pub fn delegator_kind(&self) -> &DelegatorKind {
        &self.delegator_kind
    }

    /// Returns delegatee
    pub fn validator_public_key(&self) -> &PublicKey {
        &self.validator_public_key
    }

    /// Gets the delegation rate of the provided bid
    pub fn delegation_rate(&self) -> &DelegationRate {
        &self.delegation_rate
    }

    #[cfg(any(feature = "testing", test))]
    pub fn random_for_delegator(rng: &mut TestRng, delegator_kind: DelegatorKind) -> Self {
        Self {
            delegator_kind,
            validator_public_key: rng.gen(),
            delegation_rate: rng.gen(),
        }
    }
}

impl CLTyped for Reservation {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for Reservation {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.delegator_kind.to_bytes()?);
        buffer.extend(self.validator_public_key.to_bytes()?);
        buffer.extend(self.delegation_rate.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.delegator_kind.serialized_length()
            + self.validator_public_key.serialized_length()
            + self.delegation_rate.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.delegator_kind.write_bytes(writer)?;
        self.validator_public_key.write_bytes(writer)?;
        self.delegation_rate.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for Reservation {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (delegator_kind, bytes) = DelegatorKind::from_bytes(bytes)?;
        let (validator_public_key, bytes) = PublicKey::from_bytes(bytes)?;
        let (delegation_rate, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            Self {
                delegator_kind,
                validator_public_key,
                delegation_rate,
            },
            bytes,
        ))
    }
}

impl Display for Reservation {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "Reservation {{ delegator {}, validator {} }}",
            self.delegator_kind, self.validator_public_key
        )
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<Reservation> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Reservation {
        Reservation {
            delegator_kind: rng.gen(),
            validator_public_key: rng.gen(),
            delegation_rate: rng.gen(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, system::auction::Reservation, PublicKey, SecretKey};

    #[test]
    fn serialization_roundtrip() {
        let delegator_kind = PublicKey::from(
            &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
        )
        .into();

        let validator_public_key: PublicKey = PublicKey::from(
            &SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap(),
        );
        let entry = Reservation::new(validator_public_key, delegator_kind, 0);
        bytesrepr::test_serialization_roundtrip(&entry);
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid(bid in gens::reservation_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid);
        }
    }
}
