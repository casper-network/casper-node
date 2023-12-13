//! Types for the `State::AllValues` request.

use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
use alloc::vec::Vec;

use super::type_wrappers::StoredValues;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use core::iter;

#[cfg(test)]
use crate::{testing::TestRng, ByteCode, ByteCodeKind, StoredValue};

const ROOT_NOT_FOUND_TAG: u8 = 0;
const SUCCESS_TAG: u8 = 1;

/// Represents a result of a `get_all_values` request.
#[derive(Debug, PartialEq)]
pub enum GetAllValuesResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains values returned from the global state.
    Success {
        /// Current values.
        values: StoredValues,
    },
}

impl GetAllValuesResult {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..2) {
            0 => GetAllValuesResult::RootNotFound,
            1 => {
                let count = rng.gen_range(10..20);
                GetAllValuesResult::Success {
                    values: StoredValues(
                        iter::repeat_with(|| {
                            StoredValue::ByteCode(ByteCode::new(
                                ByteCodeKind::V1CasperWasm,
                                rng.random_vec(10..20),
                            ))
                        })
                        .take(count)
                        .collect(),
                    ),
                }
            }
            _ => panic!(),
        }
    }
}

impl ToBytes for GetAllValuesResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GetAllValuesResult::RootNotFound => ROOT_NOT_FOUND_TAG.write_bytes(writer),
            GetAllValuesResult::Success { values } => {
                SUCCESS_TAG.write_bytes(writer)?;
                values.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GetAllValuesResult::RootNotFound => 0,
                GetAllValuesResult::Success { values } => values.serialized_length(),
            }
    }
}

impl FromBytes for GetAllValuesResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = FromBytes::from_bytes(bytes)?;
        match tag {
            SUCCESS_TAG => {
                let (values, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((GetAllValuesResult::Success { values }, remainder))
            }
            ROOT_NOT_FOUND_TAG => Ok((GetAllValuesResult::RootNotFound, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = GetAllValuesResult::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
