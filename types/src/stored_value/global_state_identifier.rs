use alloc::vec::Vec;

#[cfg(test)]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHash, BlockIdentifier, Digest,
};

const BLOCK_HASH_TAG: u8 = 0;
const BLOCK_HEIGHT_TAG: u8 = 1;
const STATE_ROOT_HASH_TAG: u8 = 2;

/// Identifier for possible ways to query Global State
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum GlobalStateIdentifier {
    /// Query using a block hash.
    BlockHash(BlockHash),
    /// Query using a block height.
    BlockHeight(u64),
    /// Query using the state root hash.
    StateRootHash(Digest),
}

impl GlobalStateIdentifier {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => Self::BlockHash(BlockHash::random(rng)),
            1 => Self::BlockHeight(rng.gen()),
            2 => Self::StateRootHash(Digest::random(rng)),
            _ => panic!(),
        }
    }
}

impl From<BlockIdentifier> for GlobalStateIdentifier {
    fn from(block_identifier: BlockIdentifier) -> Self {
        match block_identifier {
            BlockIdentifier::Hash(block_hash) => GlobalStateIdentifier::BlockHash(block_hash),
            BlockIdentifier::Height(block_height) => {
                GlobalStateIdentifier::BlockHeight(block_height)
            }
        }
    }
}

impl FromBytes for GlobalStateIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        match bytes.split_first() {
            Some((&BLOCK_HASH_TAG, rem)) => {
                let (block_hash, rem) = FromBytes::from_bytes(rem)?;
                Ok((GlobalStateIdentifier::BlockHash(block_hash), rem))
            }
            Some((&BLOCK_HEIGHT_TAG, rem)) => {
                let (block_height, rem) = FromBytes::from_bytes(rem)?;
                Ok((GlobalStateIdentifier::BlockHeight(block_height), rem))
            }
            Some((&STATE_ROOT_HASH_TAG, rem)) => {
                let (state_root_hash, rem) = FromBytes::from_bytes(rem)?;
                Ok((GlobalStateIdentifier::StateRootHash(state_root_hash), rem))
            }
            Some(_) | None => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl ToBytes for GlobalStateIdentifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GlobalStateIdentifier::BlockHash(block_hash) => {
                writer.push(BLOCK_HASH_TAG);
                block_hash.write_bytes(writer)?;
            }
            GlobalStateIdentifier::BlockHeight(block_height) => {
                writer.push(BLOCK_HEIGHT_TAG);
                block_height.write_bytes(writer)?;
            }
            GlobalStateIdentifier::StateRootHash(state_root_hash) => {
                writer.push(STATE_ROOT_HASH_TAG);
                state_root_hash.write_bytes(writer)?;
            }
        }
        Ok(())
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GlobalStateIdentifier::BlockHash(block_hash) => block_hash.serialized_length(),
                GlobalStateIdentifier::BlockHeight(block_height) => {
                    block_height.serialized_length()
                }
                GlobalStateIdentifier::StateRootHash(state_root_hash) => {
                    state_root_hash.serialized_length()
                }
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

        let val = GlobalStateIdentifier::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
