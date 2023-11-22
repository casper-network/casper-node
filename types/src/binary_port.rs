//! The binary port.

use serde::Serialize;

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contract_messages::Messages,
    execution::ExecutionResultV2,
    BlockHash, BlockHeader, Digest, Key, ProtocolVersion, StoredValue, Timestamp, Transaction,
    TransactionHash,
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

const HASH_TAG: u8 = 0;
const HEIGHT_TAG: u8 = 1;

/// Identifier of a chain block.
#[derive(Debug)]
pub enum BlockIdentifier {
    /// Block hash.
    Hash(BlockHash),
    /// Block height.
    Height(u64),
}

impl ToBytes for BlockIdentifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            BlockIdentifier::Hash(hash) => {
                HASH_TAG.write_bytes(writer)?;
                hash.write_bytes(writer)
            }
            BlockIdentifier::Height(height) => {
                HEIGHT_TAG.write_bytes(writer)?;
                height.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                BlockIdentifier::Hash(hash) => hash.serialized_length(),
                BlockIdentifier::Height(height) => height.serialized_length(),
            }
    }
}

impl FromBytes for BlockIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            HASH_TAG => {
                let (hash, remainder) = BlockHash::from_bytes(remainder)?;
                Ok((BlockIdentifier::Hash(hash), remainder))
            }
            HEIGHT_TAG => {
                let (hash, remainder) = u64::from_bytes(remainder)?;
                Ok((BlockIdentifier::Height(hash), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

const DB_TAG: u8 = 0;
const NON_PERSISTED_DATA_TAG: u8 = 1;
const STATE_TAG: u8 = 2;

/// The kind of the `Get` operation.
#[derive(Debug)]
pub enum GetRequest {
    /// Gets data stored under the given key from the given db.
    Db {
        /// Id of the database.
        db: DbId,
        /// Key.
        key: Vec<u8>,
    },
    /// Gets a data which is not persisted, and build on demand by the `casper-node`.
    NonPersistedData(NonPersistedDataRequest),
    /// Gets data from the global state.
    State {
        /// State root hash
        state_root_hash: Digest,
        /// Key under which data is stored.
        base_key: Key,
        /// Path under which the value is stored.
        path: Vec<String>,
    },
}

impl ToBytes for GetRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GetRequest::Db { db, key } => {
                DB_TAG.write_bytes(writer)?;
                db.write_bytes(writer)?;
                key.write_bytes(writer)
            }
            GetRequest::NonPersistedData(inner) => {
                NON_PERSISTED_DATA_TAG.write_bytes(writer)?;
                inner.write_bytes(writer)
            }
            GetRequest::State {
                state_root_hash,
                base_key,
                path,
            } => {
                STATE_TAG.write_bytes(writer)?;
                state_root_hash.write_bytes(writer)?;
                base_key.write_bytes(writer)?;
                path.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GetRequest::Db { db, key } => db.serialized_length() + key.serialized_length(),
                GetRequest::NonPersistedData(inner) => inner.serialized_length(),
                GetRequest::State {
                    state_root_hash,
                    base_key,
                    path,
                } => {
                    state_root_hash.serialized_length()
                        + base_key.serialized_length()
                        + path.serialized_length()
                }
            }
    }
}

impl FromBytes for GetRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            DB_TAG => {
                let (db, remainder) = DbId::from_bytes(remainder)?;
                let (key, remainder) = Bytes::from_bytes(remainder)?;
                Ok((
                    GetRequest::Db {
                        db,
                        key: key.into(),
                    },
                    remainder,
                ))
            }
            NON_PERSISTED_DATA_TAG => {
                let (non_persisted_data, remainder) =
                    NonPersistedDataRequest::from_bytes(remainder)?;
                Ok((GetRequest::NonPersistedData(non_persisted_data), remainder))
            }
            STATE_TAG => {
                let (state_root_hash, remainder) = Digest::from_bytes(remainder)?;
                let (base_key, remainder) = Key::from_bytes(remainder)?;
                let (path, remainder) = Vec::<String>::from_bytes(remainder)?;
                Ok((
                    GetRequest::State {
                        state_root_hash,
                        base_key,
                        path,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

const GET_TAG: u8 = 0;
const TRY_ACCEPT_TRANSACTION_TAG: u8 = 1;
const SPECULATIVE_EXEC_TAG: u8 = 2;

/// A request to the binary access interface.
// TODO[RC] Add version tag, or rather follow the `BinaryRequestV1/V2` scheme.
// TODO[RC] All `Get` operations to be unified under a single enum variant.
#[derive(Debug)]
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
    SpeculativeExec {
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
            BinaryRequest::SpeculativeExec {
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
                BinaryRequest::SpeculativeExec {
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
                    BinaryRequest::SpeculativeExec {
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
