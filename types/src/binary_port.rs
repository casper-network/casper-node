//! The binary port.

use serde::Serialize;

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHash, TransactionHash,
};

const BLOCK_HEADER_DB_TAG: u8 = 0;
const BLOCK_HEADER_V2_DB_TAG: u8 = 1;
const BLOCK_METADATA_DB_TAG: u8 = 2;
const DEPLOYS_DB_TAG: u8 = 3;
const TRANSACTIONS_DB_TAG: u8 = 4;
const DEPLOY_METADATA_DB_TAG: u8 = 5;
const EXECUTION_RESULTS_DB_TAG: u8 = 6;
const TRANSFER_DB_TAG: u8 = 7;
const STATE_STORE_DB_TAG: u8 = 8;
const BLOCKBODY_DB_TAG: u8 = 9;
const BLOCKBODY_V2_DB_TAG: u8 = 10;
const FINALIZED_APPROVALS_DB_TAG: u8 = 11;
const VERSIONED_FINALIZED_APPROVALS_DB_TAG: u8 = 12;
const APPROVALS_HASHES_DB_TAG: u8 = 13;
const VERSIONED_APPROVALS_HASHES_DB_TAG: u8 = 14;

/// Allows to indicate to which database the binary request refers to.
#[derive(Debug, Eq, PartialEq, Hash, Serialize)]
#[repr(u8)]
pub enum DbId {
    /// Refers to `BlockHeader` db.
    BlockHeader = 0,
    /// Refers to `BlockHeaderV2` db.
    BlockHeaderV2 = 1,
    /// Refers to `BlockMetadata` db.
    BlockMetadata = 2,
    /// Refers to `Deploys` db.
    Deploys = 3,
    /// Refers to `Transactions` db.
    Transactions = 4,
    /// Refers to `DeployMetadata` db.
    DeployMetadata = 5,
    /// Refers to `ExecutionResults` db.
    ExecutionResults = 6,
    /// Refers to `Transfer` db.
    Transfer = 7,
    /// Refers to `StateStore` db.
    StateStore = 8,
    /// Refers to `BlockBody` db.
    BlockBody = 9,
    /// Refers to `BlockBodyV2` db.
    BlockBodyV2 = 10,
    /// Refers to `FinalizedApprovals` db.
    FinalizedApprovals = 11,
    /// Refers to `VersionedFinalizedApprovals` db.
    VersionedFinalizedApprovals = 12,
    /// Refers to `ApprovalsHashes` db.
    ApprovalsHashes = 13,
    /// Refers to `VersionedApprovalsHashes` db.
    VersionedApprovalsHashes = 14,
}

impl std::fmt::Display for DbId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DbId::BlockHeader => write!(f, "BlockHeader"),
            DbId::BlockHeaderV2 => write!(f, "BlockHeaderV2"),
            DbId::BlockMetadata => write!(f, "BlockMetadata"),
            DbId::Deploys => write!(f, "Deploys"),
            DbId::Transactions => write!(f, "Transactions"),
            DbId::DeployMetadata => write!(f, "DeployMetadata"),
            DbId::ExecutionResults => write!(f, "ExecutionResults"),
            DbId::Transfer => write!(f, "Transfer"),
            DbId::StateStore => write!(f, "StateStore"),
            DbId::BlockBody => write!(f, "BlockBody"),
            DbId::BlockBodyV2 => write!(f, "BlockBodyV2"),
            DbId::FinalizedApprovals => write!(f, "FinalizedApprovals"),
            DbId::VersionedFinalizedApprovals => write!(f, "VersionedFinalizedApprovals"),
            DbId::ApprovalsHashes => write!(f, "ApprovalsHashes"),
            DbId::VersionedApprovalsHashes => write!(f, "VersionedApprovalsHashes"),
        }
    }
}

impl ToBytes for DbId {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            // TODO[RC]: Do this less verbosely, plausibly by serializing as u8 or using the
            // `bytesprepr derive` macro when available.
            DbId::BlockHeader => BLOCK_HEADER_DB_TAG,
            DbId::BlockHeaderV2 => BLOCK_HEADER_V2_DB_TAG,
            DbId::BlockMetadata => BLOCK_METADATA_DB_TAG,
            DbId::Deploys => DEPLOYS_DB_TAG,
            DbId::Transactions => TRANSACTIONS_DB_TAG,
            DbId::DeployMetadata => DEPLOY_METADATA_DB_TAG,
            DbId::ExecutionResults => EXECUTION_RESULTS_DB_TAG,
            DbId::Transfer => TRANSFER_DB_TAG,
            DbId::StateStore => STATE_STORE_DB_TAG,
            DbId::BlockBody => BLOCKBODY_DB_TAG,
            DbId::BlockBodyV2 => BLOCKBODY_V2_DB_TAG,
            DbId::FinalizedApprovals => FINALIZED_APPROVALS_DB_TAG,
            DbId::VersionedFinalizedApprovals => VERSIONED_FINALIZED_APPROVALS_DB_TAG,
            DbId::ApprovalsHashes => APPROVALS_HASHES_DB_TAG,
            DbId::VersionedApprovalsHashes => VERSIONED_APPROVALS_HASHES_DB_TAG,
        }
        .write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for DbId {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let db_id = match tag {
            BLOCK_HEADER_DB_TAG => DbId::BlockHeader,
            BLOCK_HEADER_V2_DB_TAG => DbId::BlockHeaderV2,
            BLOCK_METADATA_DB_TAG => DbId::BlockMetadata,
            DEPLOYS_DB_TAG => DbId::Deploys,
            TRANSACTIONS_DB_TAG => DbId::Transactions,
            DEPLOY_METADATA_DB_TAG => DbId::DeployMetadata,
            EXECUTION_RESULTS_DB_TAG => DbId::ExecutionResults,
            TRANSFER_DB_TAG => DbId::Transfer,
            STATE_STORE_DB_TAG => DbId::StateStore,
            BLOCKBODY_DB_TAG => DbId::BlockBody,
            BLOCKBODY_V2_DB_TAG => DbId::BlockBodyV2,
            FINALIZED_APPROVALS_DB_TAG => DbId::FinalizedApprovals,
            VERSIONED_FINALIZED_APPROVALS_DB_TAG => DbId::VersionedFinalizedApprovals,
            APPROVALS_HASHES_DB_TAG => DbId::ApprovalsHashes,
            VERSIONED_APPROVALS_HASHES_DB_TAG => DbId::VersionedApprovalsHashes,
            _ => return Err(bytesrepr::Error::NotRepresentable),
        };
        Ok((db_id, remainder))
    }
}

const GET_TAG: u8 = 0;
const PUT_TRANSACTION_TAG: u8 = 1;
const SPECULATIVE_EXEC_TAG: u8 = 2;
const IN_MEM_REQUEST_TAG: u8 = 3;

/// TODO
#[derive(Debug)]
pub enum BinaryRequest {
    // TODO[RC] Add version tag, or rather follow the `BinaryRequestV1/V2` scheme.
    /// Gets data stored under the given key from the given db.
    Get {
        /// Id of the database.
        db: DbId,
        /// Key.
        key: Vec<u8>,
    },
    /// Gets a data which is not persisted, hence cannot be obtained by the raw `Get` request.
    GetInMem(InMemRequest),
    /// TODO
    PutTransaction {
        /// TODO
        tbd: u32,
    },
    /// TODO
    SpeculativeExec {
        /// TODO
        tbd: u32,
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
            BinaryRequest::Get { db, key } => {
                GET_TAG.write_bytes(writer)?;
                db.write_bytes(writer)?;
                key.write_bytes(writer)
            }
            BinaryRequest::PutTransaction { tbd } => {
                PUT_TRANSACTION_TAG.write_bytes(writer)?;
                tbd.write_bytes(writer)
            }
            BinaryRequest::SpeculativeExec { tbd } => {
                SPECULATIVE_EXEC_TAG.write_bytes(writer)?;
                tbd.write_bytes(writer)
            }
            BinaryRequest::GetInMem(req) => {
                IN_MEM_REQUEST_TAG.write_bytes(writer)?;
                req.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BinaryRequest::Get { db, key } => db.serialized_length() + key.serialized_length(),
                BinaryRequest::PutTransaction { tbd } => tbd.serialized_length(),
                BinaryRequest::SpeculativeExec { tbd } => tbd.serialized_length(),
                BinaryRequest::GetInMem(req) => req.serialized_length(),
            }
    }
}

impl FromBytes for BinaryRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            GET_TAG => {
                let (db, remainder) = DbId::from_bytes(remainder)?;
                let (key, remainder) = Bytes::from_bytes(remainder)?;
                Ok((
                    BinaryRequest::Get {
                        db,
                        key: key.into(),
                    },
                    remainder,
                ))
            }
            IN_MEM_REQUEST_TAG => {
                let (in_mem_request, remainder) = InMemRequest::from_bytes(remainder)?;
                Ok((BinaryRequest::GetInMem(in_mem_request), remainder))
            }
            PUT_TRANSACTION_TAG => {
                let (tbd, remainder) = u32::from_bytes(remainder)?;
                Ok((BinaryRequest::PutTransaction { tbd }, remainder))
            }
            SPECULATIVE_EXEC_TAG => {
                let (tbd, remainder) = u32::from_bytes(remainder)?;
                Ok((BinaryRequest::SpeculativeExec { tbd }, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

const BLOCK_HEIGHT_2_HASH_TAG: u8 = 0;
const HIGHEST_BLOCK_TAG: u8 = 1;
const COMPLETED_BLOCK_CONTAINS_TAG: u8 = 2;
const TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG: u8 = 3;

/// Request for in-memory data.
#[derive(Debug)]
pub enum InMemRequest {
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

impl ToBytes for InMemRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            InMemRequest::BlockHeight2Hash { height } => {
                BLOCK_HEIGHT_2_HASH_TAG.write_bytes(writer)?;
                height.write_bytes(writer)
            }
            InMemRequest::HighestCompleteBlock => HIGHEST_BLOCK_TAG.write_bytes(writer),
            InMemRequest::CompletedBlockContains { block_hash } => {
                COMPLETED_BLOCK_CONTAINS_TAG.write_bytes(writer)?;
                block_hash.write_bytes(writer)
            }
            InMemRequest::TransactionHash2BlockHashAndHeight { transaction_hash } => {
                TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG.write_bytes(writer)?;
                transaction_hash.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                InMemRequest::BlockHeight2Hash { height } => height.serialized_length(),
                InMemRequest::HighestCompleteBlock => 0,
                InMemRequest::CompletedBlockContains { block_hash } => {
                    block_hash.serialized_length()
                }
                InMemRequest::TransactionHash2BlockHashAndHeight { transaction_hash } => {
                    transaction_hash.serialized_length()
                }
            }
    }
}

impl FromBytes for InMemRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            BLOCK_HEIGHT_2_HASH_TAG => {
                let (height, remainder) = u64::from_bytes(remainder)?;
                Ok((InMemRequest::BlockHeight2Hash { height }, remainder))
            }
            HIGHEST_BLOCK_TAG => Ok((InMemRequest::HighestCompleteBlock, remainder)),
            COMPLETED_BLOCK_CONTAINS_TAG => {
                let (block_hash, remainder) = BlockHash::from_bytes(remainder)?;
                Ok((
                    InMemRequest::CompletedBlockContains { block_hash },
                    remainder,
                ))
            }
            TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG => {
                let (transaction_hash, remainder) = TransactionHash::from_bytes(remainder)?;
                Ok((
                    InMemRequest::TransactionHash2BlockHashAndHeight { transaction_hash },
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
