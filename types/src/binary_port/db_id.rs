//! The database identifier.

use serde::Serialize;

use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

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
