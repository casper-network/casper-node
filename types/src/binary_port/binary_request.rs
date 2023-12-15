//! The request to the binary port.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHeader, Digest, ProtocolVersion, Timestamp, Transaction,
};
use alloc::vec::Vec;

use super::get::GetRequest;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::{testing::TestRng, Block, TestBlockV1Builder};

const GET_TAG: u8 = 0;
const TRY_ACCEPT_TRANSACTION_TAG: u8 = 1;
const SPECULATIVE_EXEC_TAG: u8 = 2;

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
                GET_TAG.write_bytes(writer)?;
                inner.write_bytes(writer)
            }
            BinaryRequest::TryAcceptTransaction { transaction } => {
                TRY_ACCEPT_TRANSACTION_TAG.write_bytes(writer)?;
                transaction.write_bytes(writer)
            }
            BinaryRequest::TrySpeculativeExec {
                transaction,
                state_root_hash,
                block_time,
                protocol_version,
                speculative_exec_at_block,
            } => {
                SPECULATIVE_EXEC_TAG.write_bytes(writer)?;
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
        match tag {
            GET_TAG => {
                let (get_request, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((BinaryRequest::Get(get_request), remainder))
            }
            TRY_ACCEPT_TRANSACTION_TAG => {
                let (transaction, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    BinaryRequest::TryAcceptTransaction { transaction },
                    remainder,
                ))
            }
            SPECULATIVE_EXEC_TAG => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = BinaryRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
