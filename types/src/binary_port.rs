//! The binary port.
pub mod binary_request;
pub mod db_id;
pub mod get_request;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contract_messages::Messages,
    execution::ExecutionResultV2,
    BlockHash, StoredValue, TransactionHash,
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

/// Response to the request for in-memory data.
#[derive(Debug)]
pub enum InMemResponse {
    /// Returns hash for a given height.
    BlockHeight2Hash {
        /// Block hash.
        hash: BlockHash,
    },
    /// Returns height&hash for the currently highest block.
    HighestBlock {
        /// Block hash.
        hash: BlockHash,
        /// Block height.
        height: u64,
    },
    /// Returns true if `self.completed_blocks.highest_sequence()` contains the given hash
    CompletedBlockContains(bool),
    /// Block height and hash for a given transaction.
    TransactionHash2BlockHashAndHeight {
        /// Block hash.
        hash: BlockHash,
        /// Block height.
        height: u64,
    },
}

impl ToBytes for InMemResponse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            InMemResponse::BlockHeight2Hash { hash } => {
                BLOCK_HEIGHT_2_HASH_TAG.write_bytes(writer)?;
                hash.write_bytes(writer)
            }
            InMemResponse::HighestBlock { hash, height } => {
                HIGHEST_BLOCK_TAG.write_bytes(writer)?;
                hash.write_bytes(writer)?;
                height.write_bytes(writer)
            }
            InMemResponse::CompletedBlockContains(val) => {
                COMPLETED_BLOCK_CONTAINS_TAG.write_bytes(writer)?;
                val.write_bytes(writer)
            }
            InMemResponse::TransactionHash2BlockHashAndHeight { hash, height } => {
                TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG.write_bytes(writer)?;
                hash.write_bytes(writer)?;
                height.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                InMemResponse::BlockHeight2Hash { hash } => hash.serialized_length(),
                InMemResponse::HighestBlock { hash, height } => {
                    hash.serialized_length() + height.serialized_length()
                }
                InMemResponse::CompletedBlockContains(val) => val.serialized_length(),
                InMemResponse::TransactionHash2BlockHashAndHeight { hash, height } => {
                    hash.serialized_length() + height.serialized_length()
                }
            }
    }
}

impl FromBytes for InMemResponse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            BLOCK_HEIGHT_2_HASH_TAG => {
                let (hash, remainder) = BlockHash::from_bytes(remainder)?;
                Ok((InMemResponse::BlockHeight2Hash { hash }, remainder))
            }
            HIGHEST_BLOCK_TAG => {
                let (hash, remainder) = BlockHash::from_bytes(remainder)?;
                let (height, remainder) = u64::from_bytes(remainder)?;
                Ok((InMemResponse::HighestBlock { hash, height }, remainder))
            }
            COMPLETED_BLOCK_CONTAINS_TAG => {
                let (val, remainder) = bool::from_bytes(remainder)?;
                Ok((InMemResponse::CompletedBlockContains(val), remainder))
            }
            TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG => {
                let (hash, remainder) = BlockHash::from_bytes(remainder)?;
                let (height, remainder) = u64::from_bytes(remainder)?;
                Ok((
                    InMemResponse::TransactionHash2BlockHashAndHeight { hash, height },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

const SUCCESS_TAG: u8 = 0;
const VALUE_NOT_FOUND_TAG: u8 = 1;
const ROOT_NOT_FOUND_TAG: u8 = 2;
const ERROR_TAG: u8 = 3;

/// Carries the result of the global state query.
pub enum GlobalStateQueryResult {
    /// Successful execution.
    Success {
        /// Stored value.
        value: StoredValue,
        /// Proof.
        merkle_proof: String,
    },
    /// Value has not been found.
    ValueNotFound,
    /// Root for the given state root hash not found.
    RootNotFound,
    /// Other error.
    Error(String),
}

impl ToBytes for GlobalStateQueryResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GlobalStateQueryResult::Success {
                value,
                merkle_proof,
            } => {
                SUCCESS_TAG.write_bytes(writer)?;
                value.write_bytes(writer)?;
                merkle_proof.write_bytes(writer)
            }
            GlobalStateQueryResult::ValueNotFound => VALUE_NOT_FOUND_TAG.write_bytes(writer),
            GlobalStateQueryResult::RootNotFound => ROOT_NOT_FOUND_TAG.write_bytes(writer),
            GlobalStateQueryResult::Error(err) => {
                ERROR_TAG.write_bytes(writer)?;
                err.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GlobalStateQueryResult::Success {
                    value,
                    merkle_proof,
                } => value.serialized_length() + merkle_proof.serialized_length(),
                GlobalStateQueryResult::ValueNotFound => 0,
                GlobalStateQueryResult::RootNotFound => 0,
                GlobalStateQueryResult::Error(err) => err.serialized_length(),
            }
    }
}

impl FromBytes for GlobalStateQueryResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            SUCCESS_TAG => {
                let (value, remainder) = StoredValue::from_bytes(remainder)?;
                let (merkle_proof, remainder) = String::from_bytes(remainder)?;
                Ok((
                    GlobalStateQueryResult::Success {
                        value,
                        merkle_proof,
                    },
                    remainder,
                ))
            }
            VALUE_NOT_FOUND_TAG => Ok((GlobalStateQueryResult::ValueNotFound, remainder)),
            ROOT_NOT_FOUND_TAG => Ok((GlobalStateQueryResult::RootNotFound, remainder)),
            ERROR_TAG => {
                let (error, remainder) = String::from_bytes(remainder)?;
                Ok((GlobalStateQueryResult::Error(error), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// TODO
pub struct SpeculativeExecutionResult {
    /// Result of the execution.
    pub execution_result: ExecutionResultV2,
    /// Messages emitted during execution.
    pub messages: Messages,
}

impl ToBytes for SpeculativeExecutionResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.execution_result.write_bytes(writer)?;
        self.messages.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.execution_result.serialized_length() + self.messages.serialized_length()
    }
}

impl FromBytes for SpeculativeExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (execution_result, remainder) = ExecutionResultV2::from_bytes(bytes)?;
        let (messages, remainder) = Messages::from_bytes(remainder)?;
        Ok((
            SpeculativeExecutionResult {
                execution_result,
                messages,
            },
            remainder,
        ))
    }
}

// TODO[RC]: Move these types to separate files to avoid TAG conflicts.
// const SUCCESS_TAG: u8 = 0;
// const VALUE_NOT_FOUND_TAG: u8 = 1;
// const ROOT_NOT_FOUND_TAG: u8 = 2;
// const ERROR_TAG: u8 = 3;
