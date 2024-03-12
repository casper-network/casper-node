use core::convert::TryFrom;

#[cfg(test)]
use rand::Rng;
use serde::Serialize;

#[cfg(test)]
use crate::testing::TestRng;

/// An identifier of a record type.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize)]
#[repr(u16)]
pub enum RecordId {
    /// Refers to `BlockHeader` record.
    BlockHeader = 0,
    /// Refers to `BlockBody` record.
    BlockBody = 1,
    /// Refers to `ApprovalsHashes` record.
    ApprovalsHashes = 2,
    /// Refers to `BlockMetadata` record.
    BlockMetadata = 3,
    /// Refers to `Transaction` record.
    Transaction = 4,
    /// Refers to `ExecutionResult` record.
    ExecutionResult = 5,
    /// Refers to `Transfer` record.
    Transfer = 6,
    /// Refers to `FinalizedTransactionApprovals` record.
    FinalizedTransactionApprovals = 7,
}

impl RecordId {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..8) {
            0 => RecordId::BlockHeader,
            1 => RecordId::BlockBody,
            2 => RecordId::ApprovalsHashes,
            3 => RecordId::BlockMetadata,
            4 => RecordId::Transaction,
            5 => RecordId::ExecutionResult,
            6 => RecordId::Transfer,
            7 => RecordId::FinalizedTransactionApprovals,
            _ => unreachable!(),
        }
    }
}

impl TryFrom<u16> for RecordId {
    type Error = UnknownRecordId;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(RecordId::BlockHeader),
            1 => Ok(RecordId::BlockBody),
            2 => Ok(RecordId::ApprovalsHashes),
            3 => Ok(RecordId::BlockMetadata),
            4 => Ok(RecordId::Transaction),
            5 => Ok(RecordId::ExecutionResult),
            6 => Ok(RecordId::Transfer),
            7 => Ok(RecordId::FinalizedTransactionApprovals),
            _ => Err(UnknownRecordId(value)),
        }
    }
}

impl From<RecordId> for u16 {
    fn from(value: RecordId) -> Self {
        value as u16
    }
}

impl core::fmt::Display for RecordId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RecordId::BlockHeader => write!(f, "BlockHeader"),
            RecordId::BlockBody => write!(f, "BlockBody"),
            RecordId::ApprovalsHashes => write!(f, "ApprovalsHashes"),
            RecordId::BlockMetadata => write!(f, "BlockMetadata"),
            RecordId::Transaction => write!(f, "Transaction"),
            RecordId::ExecutionResult => write!(f, "ExecutionResult"),
            RecordId::Transfer => write!(f, "Transfer"),
            RecordId::FinalizedTransactionApprovals => write!(f, "FinalizedTransactionApprovals"),
        }
    }
}

/// Error returned when trying to convert a `u16` into a `RecordId`.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownRecordId(u16);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn tag_roundtrip() {
        let rng = &mut TestRng::new();

        let val = RecordId::random(rng);
        let tag = u16::from(val);
        assert_eq!(RecordId::try_from(tag), Ok(val));
    }
}
