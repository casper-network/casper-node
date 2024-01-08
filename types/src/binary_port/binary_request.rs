//! The request to the binary port.

use core::convert::TryFrom;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHeader, Digest, ProtocolVersion, SemVer, Timestamp, Transaction,
};
use alloc::vec::Vec;

use super::get::GetRequest;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::{testing::TestRng, Block, TestBlockV1Builder};

/// The header of a binary request.
#[derive(Debug, PartialEq)]
pub struct BinaryRequestHeader {
    protocol_version: SemVer,
}

impl BinaryRequestHeader {
    /// Creates new binary request header.
    pub fn new(protocol_version: SemVer) -> Self {
        Self { protocol_version }
    }

    /// Returns the protocol version of the request.
    pub fn protocol_version(&self) -> SemVer {
        self.protocol_version
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            protocol_version: SemVer::new(rng.gen(), rng.gen(), rng.gen()),
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
        self.protocol_version.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.protocol_version.serialized_length()
    }
}

impl FromBytes for BinaryRequestHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version, remainder) = FromBytes::from_bytes(bytes)?;
        Ok((BinaryRequestHeader { protocol_version }, remainder))
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
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => Self::Get(GetRequest::random(rng)),
            1 => Self::TryAcceptTransaction {
                transaction: Transaction::random(rng),
            },
            2 => {
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
            _ => panic!(),
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
            BinaryRequest::Get(inner) => {
                writer.push(u8::from(BinaryRequestTag::Get));
                inner.write_bytes(writer)
            }
            BinaryRequest::TryAcceptTransaction { transaction } => {
                writer.push(u8::from(BinaryRequestTag::TryAcceptTransaction));
                transaction.write_bytes(writer)
            }
            BinaryRequest::TrySpeculativeExec {
                transaction,
                state_root_hash,
                block_time,
                protocol_version,
                speculative_exec_at_block,
            } => {
                writer.push(u8::from(BinaryRequestTag::TrySpeculativeExec));
                transaction.write_bytes(writer)?;
                state_root_hash.write_bytes(writer)?;
                block_time.write_bytes(writer)?;
                protocol_version.write_bytes(writer)?;
                speculative_exec_at_block.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BinaryRequest::Get(inner) => inner.serialized_length(),
                BinaryRequest::TryAcceptTransaction { transaction } => {
                    transaction.serialized_length()
                }
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

impl FromBytes for BinaryRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match BinaryRequestTag::try_from(tag) {
            Ok(BinaryRequestTag::Get) => {
                let (get_request, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((BinaryRequest::Get(get_request), remainder))
            }
            Ok(BinaryRequestTag::TryAcceptTransaction) => {
                let (transaction, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    BinaryRequest::TryAcceptTransaction { transaction },
                    remainder,
                ))
            }
            Ok(BinaryRequestTag::TrySpeculativeExec) => {
                let (transaction, remainder) = FromBytes::from_bytes(remainder)?;
                let (state_root_hash, remainder) = FromBytes::from_bytes(remainder)?;
                let (block_time, remainder) = FromBytes::from_bytes(remainder)?;
                let (protocol_version, remainder) = FromBytes::from_bytes(remainder)?;
                let (speculative_exec_at_block, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    BinaryRequest::TrySpeculativeExec {
                        transaction,
                        state_root_hash,
                        block_time,
                        protocol_version,
                        speculative_exec_at_block,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
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
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = BinaryRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
