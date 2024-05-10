use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    checksummed_hex,
    key::FromStrError,
    Key,
};

use core::{
    convert::TryFrom,
    fmt::{Debug, Display, Formatter},
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::distributions::{Distribution, Standard};
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const BLOCK_TIME_TAG: u8 = 0;
const MESSAGE_COUNT_TAG: u8 = 1;

/// Serialization tag for BlockGlobalAddr variants.
#[derive(
    Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize,
)]
#[repr(u8)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BlockGlobalAddrTag {
    #[default]
    /// Tag for block time variant.
    BlockTime = BLOCK_TIME_TAG,
    /// Tag for processing variant.
    MessageCount = MESSAGE_COUNT_TAG,
}

impl BlockGlobalAddrTag {
    /// The length in bytes of a [`BlockGlobalAddrTag`].
    pub const BLOCK_GLOBAL_ADDR_TAG_LENGTH: usize = 1;

    /// Attempts to map `BalanceHoldAddrTag` from a u8.
    pub fn try_from_u8(value: u8) -> Option<Self> {
        // TryFrom requires std, so doing this instead.
        if value == BLOCK_TIME_TAG {
            return Some(BlockGlobalAddrTag::BlockTime);
        }
        if value == MESSAGE_COUNT_TAG {
            return Some(BlockGlobalAddrTag::MessageCount);
        }
        None
    }
}

impl Display for BlockGlobalAddrTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = match self {
            BlockGlobalAddrTag::BlockTime => BLOCK_TIME_TAG,
            BlockGlobalAddrTag::MessageCount => MESSAGE_COUNT_TAG,
        };
        write!(f, "{}", base16::encode_lower(&[tag]))
    }
}

impl ToBytes for BlockGlobalAddrTag {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        Self::BLOCK_GLOBAL_ADDR_TAG_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(*self as u8);
        Ok(())
    }
}

impl FromBytes for BlockGlobalAddrTag {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        if let Some((byte, rem)) = bytes.split_first() {
            let tag = BlockGlobalAddrTag::try_from_u8(*byte).ok_or(bytesrepr::Error::Formatting)?;
            Ok((tag, rem))
        } else {
            Err(bytesrepr::Error::Formatting)
        }
    }
}

/// Address for singleton values associated to specific block. These are values which are
/// calculated or set during the execution of a block such as the block timestamp, or the
/// total count of messages emitted during the execution of the block, and so on.
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BlockGlobalAddr {
    /// Block time variant
    #[default]
    BlockTime,
    /// Message count variant.
    MessageCount,
}

impl BlockGlobalAddr {
    /// The length in bytes of a [`BlockGlobalAddr`].
    pub const BLOCK_GLOBAL_ADDR_LENGTH: usize = BlockGlobalAddrTag::BLOCK_GLOBAL_ADDR_TAG_LENGTH;

    /// How long is be the serialized value for this instance.
    pub fn serialized_length(&self) -> usize {
        Self::BLOCK_GLOBAL_ADDR_LENGTH
    }

    /// Returns the tag of this instance.
    pub fn tag(&self) -> BlockGlobalAddrTag {
        match self {
            BlockGlobalAddr::MessageCount => BlockGlobalAddrTag::MessageCount,
            BlockGlobalAddr::BlockTime => BlockGlobalAddrTag::BlockTime,
        }
    }

    /// To formatted string.
    pub fn to_formatted_string(self) -> String {
        match self {
            BlockGlobalAddr::BlockTime => base16::encode_lower(&BLOCK_TIME_TAG.to_le_bytes()),
            BlockGlobalAddr::MessageCount => base16::encode_lower(&MESSAGE_COUNT_TAG.to_le_bytes()),
        }
    }

    /// From formatted string.
    pub fn from_formatted_string(hex: &str) -> Result<Self, FromStrError> {
        let bytes = checksummed_hex::decode(hex)
            .map_err(|error| FromStrError::BlockGlobal(error.to_string()))?;
        if bytes.is_empty() {
            return Err(FromStrError::BlockGlobal(
                "bytes should not be 0 len".to_string(),
            ));
        }
        let tag_bytes = <[u8; BlockGlobalAddrTag::BLOCK_GLOBAL_ADDR_TAG_LENGTH]>::try_from(
            bytes[0..BlockGlobalAddrTag::BLOCK_GLOBAL_ADDR_TAG_LENGTH].as_ref(),
        )
        .map_err(|err| FromStrError::BlockGlobal(err.to_string()))?;
        let tag = <u8>::from_le_bytes(tag_bytes);
        let tag = BlockGlobalAddrTag::try_from_u8(tag).ok_or_else(|| {
            FromStrError::BlockGlobal("failed to parse block global addr tag".to_string())
        })?;

        // if more tags are added, extend the below logic to handle every case.
        if tag == BlockGlobalAddrTag::BlockTime {
            Ok(BlockGlobalAddr::BlockTime)
        } else if tag == BlockGlobalAddrTag::MessageCount {
            Ok(BlockGlobalAddr::MessageCount)
        } else {
            Err(FromStrError::BlockGlobal("invalid tag".to_string()))
        }
    }
}

impl ToBytes for BlockGlobalAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.push(self.tag() as u8);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.serialized_length()
    }
}

impl FromBytes for BlockGlobalAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == BlockGlobalAddrTag::BlockTime as u8 => {
                Ok((BlockGlobalAddr::BlockTime, remainder))
            }
            tag if tag == BlockGlobalAddrTag::MessageCount as u8 => {
                Ok((BlockGlobalAddr::MessageCount, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl From<BlockGlobalAddr> for Key {
    fn from(block_global_addr: BlockGlobalAddr) -> Self {
        Key::BlockGlobal(block_global_addr)
    }
}

#[cfg(any(feature = "std", test))]
impl TryFrom<Key> for BlockGlobalAddr {
    type Error = ();

    fn try_from(value: Key) -> Result<Self, Self::Error> {
        if let Key::BlockGlobal(block_global_addr) = value {
            Ok(block_global_addr)
        } else {
            Err(())
        }
    }
}

impl Display for BlockGlobalAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = self.tag();
        write!(f, "{}", tag,)
    }
}

impl Debug for BlockGlobalAddr {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        match self {
            BlockGlobalAddr::BlockTime => write!(f, "BlockTime",),
            BlockGlobalAddr::MessageCount => write!(f, "MessageCount",),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<BlockGlobalAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> BlockGlobalAddr {
        match rng.gen_range(BLOCK_TIME_TAG..=MESSAGE_COUNT_TAG) {
            BLOCK_TIME_TAG => BlockGlobalAddr::BlockTime,
            MESSAGE_COUNT_TAG => BlockGlobalAddr::MessageCount,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{block::block_global::BlockGlobalAddr, bytesrepr};

    #[test]
    fn serialization_roundtrip() {
        let addr = BlockGlobalAddr::BlockTime;
        bytesrepr::test_serialization_roundtrip(&addr);
        let addr = BlockGlobalAddr::MessageCount;
        bytesrepr::test_serialization_roundtrip(&addr);
    }
}

#[cfg(test)]
mod prop_test_gas {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_variant_gas(addr in gens::balance_hold_addr_arb()) {
            bytesrepr::test_serialization_roundtrip(&addr);
        }
    }
}
