//! The "request for non persistent data" variant of the request to the binary port.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHash, TransactionHash,
};

const BLOCK_HEIGHT_2_HASH_TAG: u8 = 0;
const HIGHEST_BLOCK_TAG: u8 = 1;
const COMPLETED_BLOCK_CONTAINS_TAG: u8 = 2;
const TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG: u8 = 3;

/// Request for data
#[derive(Debug)]
pub enum NonPersistedDataRequest {
    /// Returns hash for a given height.
    BlockHeight2Hash {
        /// Block height.
        height: u64,
    },
    /// Returns height&hash for the currently highest block.
    HighestCompleteBlock,
    /// Returns true if `self.completed_blocks.highest_sequence()` contains the given hash
    CompletedBlockContains {
        /// Block hash.
        block_hash: BlockHash,
    },
    /// Returns block hash and height for a given transaction hash.
    TransactionHash2BlockHashAndHeight {
        /// Transaction hash.
        transaction_hash: TransactionHash,
    },
}

impl ToBytes for NonPersistedDataRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            NonPersistedDataRequest::BlockHeight2Hash { height } => {
                BLOCK_HEIGHT_2_HASH_TAG.write_bytes(writer)?;
                height.write_bytes(writer)
            }
            NonPersistedDataRequest::HighestCompleteBlock => HIGHEST_BLOCK_TAG.write_bytes(writer),
            NonPersistedDataRequest::CompletedBlockContains { block_hash } => {
                COMPLETED_BLOCK_CONTAINS_TAG.write_bytes(writer)?;
                block_hash.write_bytes(writer)
            }
            NonPersistedDataRequest::TransactionHash2BlockHashAndHeight { transaction_hash } => {
                TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG.write_bytes(writer)?;
                transaction_hash.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                NonPersistedDataRequest::BlockHeight2Hash { height } => height.serialized_length(),
                NonPersistedDataRequest::HighestCompleteBlock => 0,
                NonPersistedDataRequest::CompletedBlockContains { block_hash } => {
                    block_hash.serialized_length()
                }
                NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                    transaction_hash,
                } => transaction_hash.serialized_length(),
            }
    }
}

impl FromBytes for NonPersistedDataRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            BLOCK_HEIGHT_2_HASH_TAG => {
                let (height, remainder) = u64::from_bytes(remainder)?;
                Ok((
                    NonPersistedDataRequest::BlockHeight2Hash { height },
                    remainder,
                ))
            }
            HIGHEST_BLOCK_TAG => Ok((NonPersistedDataRequest::HighestCompleteBlock, remainder)),
            COMPLETED_BLOCK_CONTAINS_TAG => {
                let (block_hash, remainder) = BlockHash::from_bytes(remainder)?;
                Ok((
                    NonPersistedDataRequest::CompletedBlockContains { block_hash },
                    remainder,
                ))
            }
            TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG => {
                let (transaction_hash, remainder) = TransactionHash::from_bytes(remainder)?;
                Ok((
                    NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                        transaction_hash,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
