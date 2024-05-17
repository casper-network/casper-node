use crate::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    CLType, CLTyped, EraId, PublicKey, U512,
};
use alloc::vec::Vec;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Validator credit record.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ValidatorCredit {
    /// Validator public key
    validator_public_key: PublicKey,
    /// The era id the credit was created.
    era_id: EraId,
    /// The credit amount.
    amount: U512,
}

impl ValidatorCredit {
    /// Returns a new instance of `[ValidatorCredit]`.
    pub fn new(validator_public_key: PublicKey, era_id: EraId, amount: U512) -> Self {
        ValidatorCredit {
            validator_public_key,
            era_id,
            amount,
        }
    }

    /// Gets the validator public key of this instance.
    pub fn validator_public_key(&self) -> &PublicKey {
        &self.validator_public_key
    }

    /// Gets the era_id of this instance.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Gets the era_id of this instance.
    pub fn amount(&self) -> U512 {
        self.amount
    }

    /// Increase the credit amount.
    pub fn increase(&mut self, additional_amount: U512) -> U512 {
        self.amount.saturating_add(additional_amount);
        self.amount
    }

    /// Creates a new empty instance of a credit, with amount 0.
    pub fn empty(validator_public_key: PublicKey, era_id: EraId) -> Self {
        Self {
            validator_public_key,
            era_id,
            amount: U512::zero(),
        }
    }
}

impl CLTyped for ValidatorCredit {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ToBytes for ValidatorCredit {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.validator_public_key.serialized_length()
            + self.era_id.serialized_length()
            + self.amount.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.validator_public_key.write_bytes(writer)?;
        self.era_id.write_bytes(writer)?;
        self.amount.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for ValidatorCredit {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (validator_public_key, remainder) = FromBytes::from_bytes(bytes)?;
        let (era_id, remainder) = FromBytes::from_bytes(remainder)?;
        let (amount, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            ValidatorCredit {
                validator_public_key,
                era_id,
                amount,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        bytesrepr, system::auction::validator_credit::ValidatorCredit, EraId, PublicKey, SecretKey,
        U512,
    };

    #[test]
    fn serialization_roundtrip() {
        let credit = ValidatorCredit {
            validator_public_key: PublicKey::from(
                &SecretKey::ed25519_from_bytes([0u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            era_id: EraId::new(0),
            amount: U512::one(),
        };
        bytesrepr::test_serialization_roundtrip(&credit);
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid(bid in gens::credit_bid_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid);
        }
    }
}
