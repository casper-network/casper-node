//! The request to the binary port.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHeader, Digest, ProtocolVersion, Timestamp, Transaction,
};

use super::get::GetRequest;

const GET_TAG: u8 = 0;
const TRY_ACCEPT_TRANSACTION_TAG: u8 = 1;
const SPECULATIVE_EXEC_TAG: u8 = 2;

/// A request to the binary access interface.
// TODO[RC] Add version tag, or rather follow the `BinaryRequestV1/V2` scheme.
// TODO[RC] Remove clone
#[derive(Clone, Debug)]
pub enum BinaryRequest {
    /// Request to get data from the node
    Get(GetRequest),
    /// Request to add a transaction into a blockchain.
    TryAcceptTransaction {
        /// Transaction to be handled.
        transaction: Transaction,
        /// Optional block header required if the request should perform speculative execution.
        speculative_exec_at_block: Option<BlockHeader>,
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
    },
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
            BinaryRequest::TryAcceptTransaction {
                transaction,
                speculative_exec_at_block,
            } => {
                TRY_ACCEPT_TRANSACTION_TAG.write_bytes(writer)?;
                transaction.write_bytes(writer)?;
                speculative_exec_at_block.write_bytes(writer)
            }
            BinaryRequest::TrySpeculativeExec {
                transaction,
                state_root_hash,
                block_time,
                protocol_version,
            } => {
                SPECULATIVE_EXEC_TAG.write_bytes(writer)?;
                transaction.write_bytes(writer)?;
                state_root_hash.write_bytes(writer)?;
                block_time.write_bytes(writer)?;
                protocol_version.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BinaryRequest::Get(inner) => inner.serialized_length(),
                BinaryRequest::TryAcceptTransaction {
                    transaction,
                    speculative_exec_at_block,
                } => {
                    transaction.serialized_length() + speculative_exec_at_block.serialized_length()
                }
                BinaryRequest::TrySpeculativeExec {
                    transaction,
                    state_root_hash,
                    block_time,
                    protocol_version,
                } => {
                    transaction.serialized_length()
                        + state_root_hash.serialized_length()
                        + block_time.serialized_length()
                        + protocol_version.serialized_length()
                }
            }
    }
}

impl FromBytes for BinaryRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            GET_TAG => {
                let (get_request, remainder) = GetRequest::from_bytes(remainder)?;
                Ok((BinaryRequest::Get(get_request), remainder))
            }
            TRY_ACCEPT_TRANSACTION_TAG => {
                let (transaction, remainder) = Transaction::from_bytes(remainder)?;
                let (speculative_exec_at_block, remainder) =
                    Option::<BlockHeader>::from_bytes(remainder)?;
                Ok((
                    BinaryRequest::TryAcceptTransaction {
                        transaction,
                        speculative_exec_at_block,
                    },
                    remainder,
                ))
            }
            SPECULATIVE_EXEC_TAG => {
                let (transaction, remainder) = Transaction::from_bytes(remainder)?;
                let (state_root_hash, remainder) = Digest::from_bytes(remainder)?;
                let (block_time, remainder) = Timestamp::from_bytes(remainder)?;
                let (protocol_version, remainder) = ProtocolVersion::from_bytes(remainder)?;
                Ok((
                    BinaryRequest::TrySpeculativeExec {
                        transaction,
                        state_root_hash,
                        block_time,
                        protocol_version,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
