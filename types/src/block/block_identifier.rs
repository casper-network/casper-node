use alloc::vec::Vec;
use core::num::ParseIntError;
#[cfg(test)]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHash, Digest, DigestError,
};

const HASH_TAG: u8 = 0;
const HEIGHT_TAG: u8 = 1;

/// Identifier for possible ways to retrieve a block.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum BlockIdentifier {
    /// Identify and retrieve the block with its hash.
    Hash(BlockHash),
    /// Identify and retrieve the block with its height.
    Height(u64),
}

impl BlockIdentifier {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..1) {
            0 => Self::Hash(BlockHash::random(rng)),
            1 => Self::Height(rng.gen()),
            _ => panic!(),
        }
    }
}

impl FromBytes for BlockIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        match bytes.split_first() {
            Some((&HASH_TAG, rem)) => {
                let (hash, rem) = FromBytes::from_bytes(rem)?;
                Ok((BlockIdentifier::Hash(hash), rem))
            }
            Some((&HEIGHT_TAG, rem)) => {
                let (height, rem) = FromBytes::from_bytes(rem)?;
                Ok((BlockIdentifier::Height(height), rem))
            }
            Some(_) | None => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl ToBytes for BlockIdentifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            BlockIdentifier::Hash(hash) => {
                writer.push(HASH_TAG);
                hash.write_bytes(writer)?;
            }
            BlockIdentifier::Height(height) => {
                writer.push(HEIGHT_TAG);
                height.write_bytes(writer)?;
            }
        }
        Ok(())
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BlockIdentifier::Hash(hash) => hash.serialized_length(),
                BlockIdentifier::Height(height) => height.serialized_length(),
            }
    }
}

impl core::str::FromStr for BlockIdentifier {
    type Err = ParseBlockIdentifierError;

    fn from_str(maybe_block_identifier: &str) -> Result<Self, Self::Err> {
        if maybe_block_identifier.is_empty() {
            return Err(ParseBlockIdentifierError::EmptyString);
        }

        if maybe_block_identifier.len() == (Digest::LENGTH * 2) {
            let hash = Digest::from_hex(maybe_block_identifier)
                .map_err(ParseBlockIdentifierError::FromHexError)?;
            Ok(BlockIdentifier::Hash(BlockHash::new(hash)))
        } else {
            let height = maybe_block_identifier
                .parse()
                .map_err(ParseBlockIdentifierError::ParseIntError)?;
            Ok(BlockIdentifier::Height(height))
        }
    }
}

/// Represents errors that can arise when parsing a [`BlockIdentifier`].
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum ParseBlockIdentifierError {
    /// String was empty.
    #[cfg_attr(
        feature = "std",
        error("Empty string is not a valid block identifier.")
    )]
    EmptyString,
    /// Couldn't parse a height value.
    #[cfg_attr(feature = "std", error("Unable to parse height from string. {0}"))]
    ParseIntError(ParseIntError),
    /// Couldn't parse a blake2bhash.
    #[cfg_attr(feature = "std", error("Unable to parse digest from string. {0}"))]
    FromHexError(DigestError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = BlockIdentifier::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
