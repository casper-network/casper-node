mod lmdb_ext;
mod temp_map;
mod versioned_databases;

mod indexed_lmdb_block_store;
mod lmdb_block_store;

use core::convert::TryFrom;
pub use indexed_lmdb_block_store::IndexedLmdbBlockStore;
pub use lmdb_block_store::LmdbBlockStore;

#[cfg(test)]
use rand::Rng;
use serde::Serialize;

#[cfg(test)]
use casper_types::testing::TestRng;

/// An identifier of db tables.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize)]
#[repr(u16)]
pub enum DbTableId {
    /// Refers to `BlockHeader` db table.
    BlockHeader = 0,
    /// Refers to `BlockBody` db table.
    BlockBody = 1,
    /// Refers to `ApprovalsHashes` db table.
    ApprovalsHashes = 2,
    /// Refers to `BlockMetadata` db table.
    BlockMetadata = 3,
    /// Refers to `Transaction` db table.
    Transaction = 4,
    /// Refers to `ExecutionResult` db table.
    ExecutionResult = 5,
    /// Refers to `Transfer` db table.
    Transfer = 6,
    /// Refers to `FinalizedTransactionApprovals` db table.
    FinalizedTransactionApprovals = 7,
}

impl DbTableId {
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..8) {
            0 => DbTableId::BlockHeader,
            1 => DbTableId::BlockBody,
            2 => DbTableId::ApprovalsHashes,
            3 => DbTableId::BlockMetadata,
            4 => DbTableId::Transaction,
            5 => DbTableId::ExecutionResult,
            6 => DbTableId::Transfer,
            7 => DbTableId::FinalizedTransactionApprovals,
            _ => unreachable!(),
        }
    }
}

impl TryFrom<u16> for DbTableId {
    type Error = UnknownDbTableId;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DbTableId::BlockHeader),
            1 => Ok(DbTableId::BlockBody),
            2 => Ok(DbTableId::ApprovalsHashes),
            3 => Ok(DbTableId::BlockMetadata),
            4 => Ok(DbTableId::Transaction),
            5 => Ok(DbTableId::ExecutionResult),
            6 => Ok(DbTableId::Transfer),
            7 => Ok(DbTableId::FinalizedTransactionApprovals),
            _ => Err(UnknownDbTableId(value)),
        }
    }
}

impl From<DbTableId> for u16 {
    fn from(value: DbTableId) -> Self {
        value as u16
    }
}

impl core::fmt::Display for DbTableId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DbTableId::BlockHeader => write!(f, "BlockHeader"),
            DbTableId::BlockBody => write!(f, "BlockBody"),
            DbTableId::ApprovalsHashes => write!(f, "ApprovalsHashes"),
            DbTableId::BlockMetadata => write!(f, "BlockMetadata"),
            DbTableId::Transaction => write!(f, "Transaction"),
            DbTableId::ExecutionResult => write!(f, "ExecutionResult"),
            DbTableId::Transfer => write!(f, "Transfer"),
            DbTableId::FinalizedTransactionApprovals => write!(f, "FinalizedTransactionApprovals"),
        }
    }
}

/// Error returned when trying to convert a `u16` into a `DbTableId`.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownDbTableId(u16);

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn tag_roundtrip() {
        let rng = &mut TestRng::new();

        let val = DbTableId::random(rng);
        let tag = u16::from(val);
        assert_eq!(DbTableId::try_from(tag), Ok(val));
    }
}
