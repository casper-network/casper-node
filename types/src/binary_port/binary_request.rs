use core::convert::TryFrom;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    BlockHeader, Digest, ProtocolVersion, Timestamp, Transaction,
};
use alloc::vec::Vec;

use super::get_request::GetRequest;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::{testing::TestRng, Block, TestBlockV1Builder};

/// The header of a binary request.
#[derive(Debug, PartialEq)]
pub struct BinaryRequestHeader {
    protocol_version: ProtocolVersion,
    type_tag: u8,
}

impl BinaryRequestHeader {
    /// Creates new binary request header.
    pub fn new(protocol_version: ProtocolVersion, type_tag: BinaryRequestTag) -> Self {
        Self {
            protocol_version,
            type_tag: type_tag.into(),
        }
    }

    /// Returns the protocol version of the request.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns the type tag of the request.
    pub fn type_tag(&self) -> u8 {
        self.type_tag
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            protocol_version: ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen()),
            type_tag: BinaryRequestTag::random(rng).into(),
        }
    }
}

impl ToBytes for BinaryRequestHeader {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.protocol_version.write_bytes(writer)?;
        self.type_tag.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.protocol_version.serialized_length() + self.type_tag.serialized_length()
    }
}

impl FromBytes for BinaryRequestHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version, remainder) = FromBytes::from_bytes(bytes)?;
        let (type_tag, remainder) = u8::from_bytes(remainder)?;
        Ok((
            BinaryRequestHeader {
                protocol_version,
                type_tag,
            },
            remainder,
        ))
    }
}

/// A request to the binary access interface.
#[derive(Debug, PartialEq)]
pub enum BinaryRequest {
    /// Request to get data from the node
    Get(GetRequest),
    /// Request to add a transaction into a blockchain.
    TryAcceptTransaction {
        /// Transaction to be handled.
        transaction: Transaction,
    },
    /// Request to execute a transaction speculatively.
    TrySpeculativeExec {
        /// State root on top of which to execute deploy.
        state_root_hash: Digest,
        /// Block time.
        block_time: Timestamp,
        /// Protocol version used when creating the original block.
        protocol_version: ProtocolVersion,
        /// Transaction to execute.
        transaction: Transaction,
        /// Block header of block at which we should perform speculative execution.
        speculative_exec_at_block: BlockHeader,
    },
}

impl BinaryRequest {
    /// Returns the type tag of the request.
    pub fn tag(&self) -> BinaryRequestTag {
        match self {
            BinaryRequest::Get(_) => BinaryRequestTag::Get,
            BinaryRequest::TryAcceptTransaction { .. } => BinaryRequestTag::TryAcceptTransaction,
            BinaryRequest::TrySpeculativeExec { .. } => BinaryRequestTag::TrySpeculativeExec,
        }
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match BinaryRequestTag::random(rng) {
            BinaryRequestTag::Get => Self::Get(GetRequest::random(rng)),
            BinaryRequestTag::TryAcceptTransaction => Self::TryAcceptTransaction {
                transaction: Transaction::random(rng),
            },
            BinaryRequestTag::TrySpeculativeExec => {
                let block_v1 = TestBlockV1Builder::new().build(rng);
                let block = Block::V1(block_v1);

                Self::TrySpeculativeExec {
                    state_root_hash: Digest::random(rng),
                    block_time: Timestamp::random(rng),
                    protocol_version: ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen()),
                    transaction: Transaction::random(rng),
                    speculative_exec_at_block: block.take_header(),
                }
            }
        }
    }
}

impl ToBytes for BinaryRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            BinaryRequest::Get(inner) => inner.write_bytes(writer),
            BinaryRequest::TryAcceptTransaction { transaction } => transaction.write_bytes(writer),
            BinaryRequest::TrySpeculativeExec {
                transaction,
                state_root_hash,
                block_time,
                protocol_version,
                speculative_exec_at_block,
            } => {
                transaction.write_bytes(writer)?;
                state_root_hash.write_bytes(writer)?;
                block_time.write_bytes(writer)?;
                protocol_version.write_bytes(writer)?;
                speculative_exec_at_block.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            BinaryRequest::Get(inner) => inner.serialized_length(),
            BinaryRequest::TryAcceptTransaction { transaction } => transaction.serialized_length(),
            BinaryRequest::TrySpeculativeExec {
                transaction,
                state_root_hash,
                block_time,
                protocol_version,
                speculative_exec_at_block,
            } => {
                transaction.serialized_length()
                    + state_root_hash.serialized_length()
                    + block_time.serialized_length()
                    + protocol_version.serialized_length()
                    + speculative_exec_at_block.serialized_length()
            }
        }
    }
}

impl TryFrom<(BinaryRequestTag, &[u8])> for BinaryRequest {
    type Error = bytesrepr::Error;

    fn try_from((tag, bytes): (BinaryRequestTag, &[u8])) -> Result<Self, Self::Error> {
        let (req, remainder) = match tag {
            BinaryRequestTag::Get => {
                let (get_request, remainder) = FromBytes::from_bytes(bytes)?;
                (BinaryRequest::Get(get_request), remainder)
            }
            BinaryRequestTag::TryAcceptTransaction => {
                let (transaction, remainder) = FromBytes::from_bytes(bytes)?;
                (
                    BinaryRequest::TryAcceptTransaction { transaction },
                    remainder,
                )
            }
            BinaryRequestTag::TrySpeculativeExec => {
                let (transaction, remainder) = FromBytes::from_bytes(bytes)?;
                let (state_root_hash, remainder) = FromBytes::from_bytes(remainder)?;
                let (block_time, remainder) = FromBytes::from_bytes(remainder)?;
                let (protocol_version, remainder) = FromBytes::from_bytes(remainder)?;
                let (speculative_exec_at_block, remainder) = FromBytes::from_bytes(remainder)?;
                (
                    BinaryRequest::TrySpeculativeExec {
                        transaction,
                        state_root_hash,
                        block_time,
                        protocol_version,
                        speculative_exec_at_block,
                    },
                    remainder,
                )
            }
        };
        if !remainder.is_empty() {
            return Err(bytesrepr::Error::LeftOverBytes);
        }
        Ok(req)
    }
}

/// The type tag of a binary request.
#[derive(Debug, PartialEq)]
#[repr(u8)]
pub enum BinaryRequestTag {
    /// Request to get data from the node
    Get = 0,
    /// Request to add a transaction into a blockchain.
    TryAcceptTransaction = 1,
    /// Request to execute a transaction speculatively.
    TrySpeculativeExec = 2,
}

impl BinaryRequestTag {
    /// Creates a random `BinaryRequestTag`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => BinaryRequestTag::Get,
            1 => BinaryRequestTag::TryAcceptTransaction,
            2 => BinaryRequestTag::TrySpeculativeExec,
            _ => unreachable!(),
        }
    }
}

impl TryFrom<u8> for BinaryRequestTag {
    type Error = InvalidBinaryRequestTag;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BinaryRequestTag::Get),
            1 => Ok(BinaryRequestTag::TryAcceptTransaction),
            2 => Ok(BinaryRequestTag::TrySpeculativeExec),
            _ => Err(InvalidBinaryRequestTag(value)),
        }
    }
}

impl From<BinaryRequestTag> for u8 {
    fn from(value: BinaryRequestTag) -> Self {
        value as u8
    }
}

/// Error raised when trying to convert an invalid u8 into a `BinaryRequestTag`.
pub struct InvalidBinaryRequestTag(u8);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn header_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = BinaryRequestHeader::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }

    #[test]
    fn request_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = BinaryRequest::random(rng);
        let bytes = val.to_bytes().expect("should serialize");
        assert_eq!(BinaryRequest::try_from((val.tag(), &bytes[..])), Ok(val));
    }
}
