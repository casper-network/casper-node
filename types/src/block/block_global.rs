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
    fmt::{Display, Formatter},
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
const PROTOCOL_VERSION_TAG: u8 = 2;
const ADDRESSABLE_ENTITY_TAG: u8 = 3;
const BLOCK_PARENT_HASH_TAG: u8 = 4;

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
    /// Tag for protocol version variant.
    ProtocolVersion = PROTOCOL_VERSION_TAG,
    /// Tag for addressable entity variant.
    AddressableEntity = ADDRESSABLE_ENTITY_TAG,
    /// Tag for parent block hash variant.
    BlockParentHash = BLOCK_PARENT_HASH_TAG,
}

impl BlockGlobalAddrTag {
    /// The length in bytes of a [`BlockGlobalAddrTag`].
    pub const BLOCK_GLOBAL_ADDR_TAG_LENGTH: usize = 1;

    /// Attempts to map a `u8` to a `BlockGlobalAddrTag`.
    pub fn try_from_u8(value: u8) -> Option<Self> {
        // TryFrom requires std, so doing this instead.
        if value == BLOCK_TIME_TAG {
            return Some(BlockGlobalAddrTag::BlockTime);
        }
        if value == MESSAGE_COUNT_TAG {
            return Some(BlockGlobalAddrTag::MessageCount);
        }
        if value == PROTOCOL_VERSION_TAG {
            return Some(BlockGlobalAddrTag::ProtocolVersion);
        }
        if value == ADDRESSABLE_ENTITY_TAG {
            return Some(BlockGlobalAddrTag::AddressableEntity);
        }
        if value == BLOCK_PARENT_HASH_TAG {
            return Some(BlockGlobalAddrTag::BlockParentHash);
        }
        None
    }
}

impl Display for BlockGlobalAddrTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = match self {
            BlockGlobalAddrTag::BlockTime => BLOCK_TIME_TAG,
            BlockGlobalAddrTag::MessageCount => MESSAGE_COUNT_TAG,
            BlockGlobalAddrTag::ProtocolVersion => PROTOCOL_VERSION_TAG,
            BlockGlobalAddrTag::AddressableEntity => ADDRESSABLE_ENTITY_TAG,
            BlockGlobalAddrTag::BlockParentHash => BLOCK_PARENT_HASH_TAG,
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
#[derive(
    Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BlockGlobalAddr {
    /// Block time variant
    #[default]
    BlockTime,
    /// Message count variant.
    MessageCount,
    /// Protocol version.
    ProtocolVersion,
    /// Addressable entity.
    AddressableEntity,
    /// Parent block hash at a slot in a block-global ring buffer.
    BlockParentHash {
        /// Slot in the ring buffer.
        slot: u64,
    },
}

impl BlockGlobalAddr {
    /// The serialized length of a tag-only [`BlockGlobalAddr`].
    pub const BLOCK_GLOBAL_ADDR_LENGTH: usize = BlockGlobalAddrTag::BLOCK_GLOBAL_ADDR_TAG_LENGTH;

    /// The serialized length of a [`BlockGlobalAddr::BlockParentHash`].
    pub const BLOCK_PARENT_HASH_ADDR_LENGTH: usize = 32;

    /// Returns the tag of this instance.
    pub fn tag(&self) -> BlockGlobalAddrTag {
        match self {
            BlockGlobalAddr::MessageCount => BlockGlobalAddrTag::MessageCount,
            BlockGlobalAddr::BlockTime => BlockGlobalAddrTag::BlockTime,
            BlockGlobalAddr::ProtocolVersion => BlockGlobalAddrTag::ProtocolVersion,
            BlockGlobalAddr::AddressableEntity => BlockGlobalAddrTag::AddressableEntity,
            BlockGlobalAddr::BlockParentHash { .. } => BlockGlobalAddrTag::BlockParentHash,
        }
    }

    /// To formatted string.
    pub fn to_formatted_string(self) -> String {
        match self {
            BlockGlobalAddr::BlockTime => base16::encode_lower(&BLOCK_TIME_TAG.to_le_bytes()),
            BlockGlobalAddr::MessageCount => base16::encode_lower(&MESSAGE_COUNT_TAG.to_le_bytes()),
            BlockGlobalAddr::ProtocolVersion => {
                base16::encode_lower(&PROTOCOL_VERSION_TAG.to_le_bytes())
            }
            BlockGlobalAddr::AddressableEntity => {
                base16::encode_lower(&ADDRESSABLE_ENTITY_TAG.to_le_bytes())
            }
            BlockGlobalAddr::BlockParentHash { slot } => {
                let mut formatted = base16::encode_lower(&BLOCK_PARENT_HASH_TAG.to_le_bytes());
                formatted.push_str(&base16::encode_lower(&slot.to_be_bytes()));
                formatted
            }
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

        match tag {
            BlockGlobalAddrTag::BlockTime => Ok(BlockGlobalAddr::BlockTime),
            BlockGlobalAddrTag::MessageCount => Ok(BlockGlobalAddr::MessageCount),
            BlockGlobalAddrTag::ProtocolVersion => Ok(BlockGlobalAddr::ProtocolVersion),
            BlockGlobalAddrTag::AddressableEntity => Ok(BlockGlobalAddr::AddressableEntity),
            BlockGlobalAddrTag::BlockParentHash => {
                let slot_bytes = <[u8; core::mem::size_of::<u64>()]>::try_from(&bytes[1..])
                    .map_err(|error| FromStrError::BlockGlobal(error.to_string()))?;
                Ok(BlockGlobalAddr::BlockParentHash {
                    slot: u64::from_be_bytes(slot_bytes),
                })
            }
        }
    }
}

impl ToBytes for BlockGlobalAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        match self {
            BlockGlobalAddr::BlockParentHash { .. } => Self::BLOCK_PARENT_HASH_ADDR_LENGTH,
            BlockGlobalAddr::BlockTime
            | BlockGlobalAddr::MessageCount
            | BlockGlobalAddr::ProtocolVersion
            | BlockGlobalAddr::AddressableEntity => Self::BLOCK_GLOBAL_ADDR_LENGTH,
        }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            BlockGlobalAddr::BlockParentHash { slot } => {
                let mut bytes = [0u8; Self::BLOCK_PARENT_HASH_ADDR_LENGTH];
                bytes[0] = self.tag() as u8;
                bytes[Self::BLOCK_PARENT_HASH_ADDR_LENGTH - core::mem::size_of::<u64>()..]
                    .copy_from_slice(&slot.to_be_bytes());
                writer.extend_from_slice(&bytes);
            }
            BlockGlobalAddr::BlockTime
            | BlockGlobalAddr::MessageCount
            | BlockGlobalAddr::ProtocolVersion
            | BlockGlobalAddr::AddressableEntity => writer.push(self.tag() as u8),
        }
        Ok(())
    }
}

impl FromBytes for BlockGlobalAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            tag if tag == BlockGlobalAddrTag::BlockTime as u8 => {
                Ok((BlockGlobalAddr::BlockTime, remainder))
            }
            tag if tag == BlockGlobalAddrTag::MessageCount as u8 => {
                Ok((BlockGlobalAddr::MessageCount, remainder))
            }
            tag if tag == BlockGlobalAddrTag::ProtocolVersion as u8 => {
                Ok((BlockGlobalAddr::ProtocolVersion, remainder))
            }
            tag if tag == BlockGlobalAddrTag::AddressableEntity as u8 => {
                Ok((BlockGlobalAddr::AddressableEntity, remainder))
            }
            tag if tag == BlockGlobalAddrTag::BlockParentHash as u8 => {
                if bytes.len() < Self::BLOCK_PARENT_HASH_ADDR_LENGTH {
                    return Err(bytesrepr::Error::EarlyEndOfStream);
                }
                let (serialized_addr, remainder) =
                    bytes.split_at(Self::BLOCK_PARENT_HASH_ADDR_LENGTH);
                let slot_offset = Self::BLOCK_PARENT_HASH_ADDR_LENGTH - core::mem::size_of::<u64>();
                if serialized_addr[BlockGlobalAddrTag::BLOCK_GLOBAL_ADDR_TAG_LENGTH..slot_offset]
                    .iter()
                    .any(|byte| *byte != 0)
                {
                    return Err(bytesrepr::Error::Formatting);
                }
                let slot_bytes =
                    <[u8; core::mem::size_of::<u64>()]>::try_from(&serialized_addr[slot_offset..])
                        .map_err(|_| bytesrepr::Error::Formatting)?;
                let slot = u64::from_be_bytes(slot_bytes);
                Ok((BlockGlobalAddr::BlockParentHash { slot }, remainder))
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
        match self {
            BlockGlobalAddr::BlockParentHash { slot } => write!(f, "{}-{}", self.tag(), slot),
            BlockGlobalAddr::BlockTime => write!(f, "{}", self.tag()),
            BlockGlobalAddr::MessageCount => write!(f, "{}", self.tag()),
            BlockGlobalAddr::ProtocolVersion => write!(f, "{}", self.tag()),
            BlockGlobalAddr::AddressableEntity => write!(f, "{}", self.tag()),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<BlockGlobalAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> BlockGlobalAddr {
        match rng.gen_range(BLOCK_TIME_TAG..=BLOCK_PARENT_HASH_TAG) {
            BLOCK_TIME_TAG => BlockGlobalAddr::BlockTime,
            MESSAGE_COUNT_TAG => BlockGlobalAddr::MessageCount,
            PROTOCOL_VERSION_TAG => BlockGlobalAddr::ProtocolVersion,
            ADDRESSABLE_ENTITY_TAG => BlockGlobalAddr::AddressableEntity,
            BLOCK_PARENT_HASH_TAG => BlockGlobalAddr::BlockParentHash { slot: rng.gen() },
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        block::block_global::BlockGlobalAddr,
        bytesrepr::{self, FromBytes, ToBytes},
    };

    #[test]
    fn serialization_roundtrip() {
        let addr = BlockGlobalAddr::BlockTime;
        bytesrepr::test_serialization_roundtrip(&addr);
        let addr = BlockGlobalAddr::MessageCount;
        bytesrepr::test_serialization_roundtrip(&addr);
        let addr = BlockGlobalAddr::ProtocolVersion;
        bytesrepr::test_serialization_roundtrip(&addr);
        let addr = BlockGlobalAddr::AddressableEntity;
        bytesrepr::test_serialization_roundtrip(&addr);
        let addr = BlockGlobalAddr::BlockParentHash {
            slot: 0x0102_0304_0506_0708,
        };
        bytesrepr::test_serialization_roundtrip(&addr);
    }

    #[test]
    fn legacy_variants_keep_tag_only_serialization() {
        let variants = [
            (BlockGlobalAddr::BlockTime, 0),
            (BlockGlobalAddr::MessageCount, 1),
            (BlockGlobalAddr::ProtocolVersion, 2),
            (BlockGlobalAddr::AddressableEntity, 3),
        ];

        for (addr, tag) in variants {
            assert_eq!(addr.to_bytes().unwrap(), vec![tag]);
            assert_eq!(addr.serialized_length(), 1);

            let bytes = [tag, 0xaa, 0xbb];
            let (decoded, remainder) = BlockGlobalAddr::from_bytes(&bytes).unwrap();
            assert_eq!(decoded, addr);
            assert_eq!(remainder, &[0xaa, 0xbb]);
        }
    }

    #[test]
    fn block_parent_hash_has_canonical_bytes_and_formatted_string() {
        let addr = BlockGlobalAddr::BlockParentHash {
            slot: 0x0102_0304_0506_0708,
        };

        assert_eq!(addr.to_bytes().unwrap(), {
            let mut expected = vec![0x04];
            expected.extend_from_slice(&[0u8; 23]);
            expected.extend_from_slice(&0x0102_0304_0506_0708u64.to_be_bytes());
            expected
        });
        assert_eq!(addr.to_formatted_string(), "040102030405060708");
        assert_eq!(
            BlockGlobalAddr::from_formatted_string(&addr.to_formatted_string()).unwrap(),
            addr
        );
    }

    #[test]
    fn block_parent_hash_rejects_invalid_fixed_width_payloads() {
        let addr = BlockGlobalAddr::BlockParentHash { slot: u64::MAX };
        let mut bytes = addr.to_bytes().unwrap();

        assert_eq!(bytes.len(), BlockGlobalAddr::BLOCK_PARENT_HASH_ADDR_LENGTH);
        assert_eq!(&bytes[24..], &u64::MAX.to_be_bytes());

        bytes[1] = 1;
        assert_eq!(
            BlockGlobalAddr::from_bytes(&bytes).unwrap_err(),
            bytesrepr::Error::Formatting
        );

        let encoded = addr.to_bytes().unwrap();
        let truncated = &encoded[..31];
        assert_eq!(
            BlockGlobalAddr::from_bytes(truncated).unwrap_err(),
            bytesrepr::Error::EarlyEndOfStream
        );
    }
}

#[cfg(test)]
mod prop_test_gas {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn serialization_roundtrip(addr in gens::block_global_addr_arb()) {
            bytesrepr::test_serialization_roundtrip(&addr);
        }
    }
}
