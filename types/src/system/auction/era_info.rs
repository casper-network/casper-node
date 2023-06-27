use alloc::{boxed::Box, vec::Vec};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey, U512,
};

const SEIGNIORAGE_ALLOCATION_VALIDATOR_TAG: u8 = 0;
const SEIGNIORAGE_ALLOCATION_DELEGATOR_TAG: u8 = 1;

/// Information about a seigniorage allocation
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum SeigniorageAllocation {
    /// Info about a seigniorage allocation for a validator
    Validator {
        /// Validator's public key
        validator_public_key: PublicKey,
        /// Allocated amount
        amount: U512,
    },
    /// Info about a seigniorage allocation for a delegator
    Delegator {
        /// Delegator's public key
        delegator_public_key: PublicKey,
        /// Validator's public key
        validator_public_key: PublicKey,
        /// Allocated amount
        amount: U512,
    },
}

impl SeigniorageAllocation {
    /// Constructs a [`SeigniorageAllocation::Validator`]
    pub const fn validator(validator_public_key: PublicKey, amount: U512) -> Self {
        SeigniorageAllocation::Validator {
            validator_public_key,
            amount,
        }
    }

    /// Constructs a [`SeigniorageAllocation::Delegator`]
    pub const fn delegator(
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
    ) -> Self {
        SeigniorageAllocation::Delegator {
            delegator_public_key,
            validator_public_key,
            amount,
        }
    }

    /// Returns the amount for a given seigniorage allocation
    pub fn amount(&self) -> &U512 {
        match self {
            SeigniorageAllocation::Validator { amount, .. } => amount,
            SeigniorageAllocation::Delegator { amount, .. } => amount,
        }
    }

    fn tag(&self) -> u8 {
        match self {
            SeigniorageAllocation::Validator { .. } => SEIGNIORAGE_ALLOCATION_VALIDATOR_TAG,
            SeigniorageAllocation::Delegator { .. } => SEIGNIORAGE_ALLOCATION_DELEGATOR_TAG,
        }
    }
}

impl ToBytes for SeigniorageAllocation {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.tag().serialized_length()
            + match self {
                SeigniorageAllocation::Validator {
                    validator_public_key,
                    amount,
                } => validator_public_key.serialized_length() + amount.serialized_length(),
                SeigniorageAllocation::Delegator {
                    delegator_public_key,
                    validator_public_key,
                    amount,
                } => {
                    delegator_public_key.serialized_length()
                        + validator_public_key.serialized_length()
                        + amount.serialized_length()
                }
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        match self {
            SeigniorageAllocation::Validator {
                validator_public_key,
                amount,
            } => {
                validator_public_key.write_bytes(writer)?;
                amount.write_bytes(writer)?;
            }
            SeigniorageAllocation::Delegator {
                delegator_public_key,
                validator_public_key,
                amount,
            } => {
                delegator_public_key.write_bytes(writer)?;
                validator_public_key.write_bytes(writer)?;
                amount.write_bytes(writer)?;
            }
        }
        Ok(())
    }
}

impl FromBytes for SeigniorageAllocation {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = <u8>::from_bytes(bytes)?;
        match tag {
            SEIGNIORAGE_ALLOCATION_VALIDATOR_TAG => {
                let (validator_public_key, rem) = PublicKey::from_bytes(rem)?;
                let (amount, rem) = U512::from_bytes(rem)?;
                Ok((
                    SeigniorageAllocation::validator(validator_public_key, amount),
                    rem,
                ))
            }
            SEIGNIORAGE_ALLOCATION_DELEGATOR_TAG => {
                let (delegator_public_key, rem) = PublicKey::from_bytes(rem)?;
                let (validator_public_key, rem) = PublicKey::from_bytes(rem)?;
                let (amount, rem) = U512::from_bytes(rem)?;
                Ok((
                    SeigniorageAllocation::delegator(
                        delegator_public_key,
                        validator_public_key,
                        amount,
                    ),
                    rem,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl CLTyped for SeigniorageAllocation {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

/// Auction metadata.  Intended to be recorded at each era.
#[derive(Debug, Default, Clone, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EraInfo {
    seigniorage_allocations: Vec<SeigniorageAllocation>,
}

impl EraInfo {
    /// Constructs a [`EraInfo`].
    pub fn new() -> Self {
        let seigniorage_allocations = Vec::new();
        EraInfo {
            seigniorage_allocations,
        }
    }

    /// Returns a reference to the seigniorage allocations collection
    pub fn seigniorage_allocations(&self) -> &Vec<SeigniorageAllocation> {
        &self.seigniorage_allocations
    }

    /// Returns a mutable reference to the seigniorage allocations collection
    pub fn seigniorage_allocations_mut(&mut self) -> &mut Vec<SeigniorageAllocation> {
        &mut self.seigniorage_allocations
    }

    /// Returns all seigniorage allocations that match the provided public key
    /// using the following criteria:
    /// * If the match candidate is a validator allocation, the provided public key is matched
    ///   against the validator public key.
    /// * If the match candidate is a delegator allocation, the provided public key is matched
    ///   against the delegator public key.
    pub fn select(&self, public_key: PublicKey) -> impl Iterator<Item = &SeigniorageAllocation> {
        self.seigniorage_allocations
            .iter()
            .filter(move |allocation| match allocation {
                SeigniorageAllocation::Validator {
                    validator_public_key,
                    ..
                } => public_key == *validator_public_key,
                SeigniorageAllocation::Delegator {
                    delegator_public_key,
                    ..
                } => public_key == *delegator_public_key,
            })
    }
}

impl ToBytes for EraInfo {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.seigniorage_allocations().write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.seigniorage_allocations.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.seigniorage_allocations().write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for EraInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (seigniorage_allocations, rem) = Vec::<SeigniorageAllocation>::from_bytes(bytes)?;
        Ok((
            EraInfo {
                seigniorage_allocations,
            },
            rem,
        ))
    }
}

impl CLTyped for EraInfo {
    fn cl_type() -> CLType {
        CLType::List(Box::new(SeigniorageAllocation::cl_type()))
    }
}

/// Generators for [`SeigniorageAllocation`] and [`EraInfo`]
#[cfg(any(feature = "testing", feature = "gens", test))]
pub mod gens {
    use proptest::{
        collection::{self, SizeRange},
        prelude::Strategy,
        prop_oneof,
    };

    use crate::{
        crypto::gens::public_key_arb,
        gens::u512_arb,
        system::auction::{EraInfo, SeigniorageAllocation},
    };

    fn seigniorage_allocation_validator_arb() -> impl Strategy<Value = SeigniorageAllocation> {
        (public_key_arb(), u512_arb()).prop_map(|(validator_public_key, amount)| {
            SeigniorageAllocation::validator(validator_public_key, amount)
        })
    }

    fn seigniorage_allocation_delegator_arb() -> impl Strategy<Value = SeigniorageAllocation> {
        (public_key_arb(), public_key_arb(), u512_arb()).prop_map(
            |(delegator_public_key, validator_public_key, amount)| {
                SeigniorageAllocation::delegator(delegator_public_key, validator_public_key, amount)
            },
        )
    }

    /// Creates an arbitrary [`SeignorageAllocation`](crate::system::auction::SeigniorageAllocation)
    pub fn seigniorage_allocation_arb() -> impl Strategy<Value = SeigniorageAllocation> {
        prop_oneof![
            seigniorage_allocation_validator_arb(),
            seigniorage_allocation_delegator_arb()
        ]
    }

    /// Creates an arbitrary [`EraInfo`]
    pub fn era_info_arb(size: impl Into<SizeRange>) -> impl Strategy<Value = EraInfo> {
        collection::vec(seigniorage_allocation_arb(), size).prop_map(|allocations| {
            let mut era_info = EraInfo::new();
            *era_info.seigniorage_allocations_mut() = allocations;
            era_info
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::bytesrepr;

    use super::gens;

    proptest! {
        #[test]
        fn test_serialization_roundtrip(era_info in gens::era_info_arb(0..32)) {
            bytesrepr::test_serialization_roundtrip(&era_info)
        }
    }
}
