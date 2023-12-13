//! The database identifier.

use serde::Serialize;

use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
use alloc::vec::Vec;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

const BLOCK_HEADER_DB_TAG: u8 = 0;
const BLOCK_METADATA_DB_TAG: u8 = 1;
const TRANSFER_DB_TAG: u8 = 2;
const STATE_STORE_DB_TAG: u8 = 3;
const BLOCK_BODY_DB_TAG: u8 = 4;
const FINALIZED_TRANSACTION_APPROVALS_DB_TAG: u8 = 5;
const APPROVALS_HASHES_DB_TAG: u8 = 6;
const TRANSACTION_DB_TAG: u8 = 7;
const EXECUTION_RESULT_DB_TAG: u8 = 8;

/// Allows to indicate to which database the binary request refers to.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize)]
#[repr(u8)]
pub enum DbId {
    /// Refers to `BlockHeader` db.
    BlockHeader = 0,
    /// Refers to `BlockBody` db.
    BlockBody = 1,
    /// Refers to `ApprovalsHashes` db.
    ApprovalsHashes = 2,
    /// Refers to `BlockMetadata` db.
    BlockMetadata = 3,
    /// Refers to `Transaction` db.
    Transaction = 4,
    /// Refers to `ExecutionResult` db.
    ExecutionResult = 5,
    /// Refers to `Transfer` db.
    Transfer = 6,
    /// Refers to `StateStore` db.
    StateStore = 7,
    /// Refers to `FinalizedTransactionApprovals` db.
    FinalizedTransactionApprovals = 8,
}

impl DbId {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..9) {
            0 => DbId::BlockHeader,
            1 => DbId::BlockBody,
            2 => DbId::ApprovalsHashes,
            3 => DbId::BlockMetadata,
            4 => DbId::Transaction,
            5 => DbId::ExecutionResult,
            6 => DbId::Transfer,
            7 => DbId::StateStore,
            8 => DbId::FinalizedTransactionApprovals,
            _ => panic!(),
        }
    }
}

impl core::fmt::Display for DbId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DbId::BlockHeader => write!(f, "BlockHeader"),
            DbId::BlockBody => write!(f, "BlockBody"),
            DbId::ApprovalsHashes => write!(f, "ApprovalsHashes"),
            DbId::BlockMetadata => write!(f, "BlockMetadata"),
            DbId::Transaction => write!(f, "Transaction"),
            DbId::ExecutionResult => write!(f, "ExecutionResult"),
            DbId::Transfer => write!(f, "Transfer"),
            DbId::StateStore => write!(f, "StateStore"),
            DbId::FinalizedTransactionApprovals => write!(f, "FinalizedTransactionApprovals"),
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
            DbId::BlockHeader => BLOCK_HEADER_DB_TAG,
            DbId::BlockMetadata => BLOCK_METADATA_DB_TAG,
            DbId::Transfer => TRANSFER_DB_TAG,
            DbId::StateStore => STATE_STORE_DB_TAG,
            DbId::BlockBody => BLOCK_BODY_DB_TAG,
            DbId::FinalizedTransactionApprovals => FINALIZED_TRANSACTION_APPROVALS_DB_TAG,
            DbId::ApprovalsHashes => APPROVALS_HASHES_DB_TAG,
            DbId::Transaction => TRANSACTION_DB_TAG,
            DbId::ExecutionResult => EXECUTION_RESULT_DB_TAG,
        }
        .write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for DbId {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = FromBytes::from_bytes(bytes)?;
        let db_id = match tag {
            BLOCK_HEADER_DB_TAG => DbId::BlockHeader,
            BLOCK_METADATA_DB_TAG => DbId::BlockMetadata,
            TRANSFER_DB_TAG => DbId::Transfer,
            STATE_STORE_DB_TAG => DbId::StateStore,
            BLOCK_BODY_DB_TAG => DbId::BlockBody,
            FINALIZED_TRANSACTION_APPROVALS_DB_TAG => DbId::FinalizedTransactionApprovals,
            APPROVALS_HASHES_DB_TAG => DbId::ApprovalsHashes,
            TRANSACTION_DB_TAG => DbId::Transaction,
            EXECUTION_RESULT_DB_TAG => DbId::ExecutionResult,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((db_id, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = DbId::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
