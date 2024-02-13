//! The payload type.

#[cfg(feature = "json-schema")]
use schemars::JsonSchema;

#[cfg(test)]
use rand::Rng;

use alloc::vec::Vec;
use core::{convert::TryFrom, fmt};

#[cfg(test)]
use crate::testing::TestRng;

#[cfg(any(feature = "std", test))]
use super::NodeStatus;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    execution::{ExecutionResult, ExecutionResultV1},
    AvailableBlockRange, BlockBody, BlockBodyV1, BlockHeader, BlockHeaderV1, BlockSignatures,
    BlockSynchronizerStatus, Deploy, FinalizedApprovals, FinalizedDeployApprovals, Peers,
    ReactorState, SignedBlock, StoredValue, Transaction, Transfer,
};
#[cfg(any(feature = "std", test))]
use crate::{ChainspecRawBytes, NextUpgrade};

use super::{
    global_state_query_result::GlobalStateQueryResult,
    record_id::RecordId,
    type_wrappers::{
        ConsensusStatus, ConsensusValidatorChanges, GetTrieFullResult, LastProgress, NetworkName,
        SpeculativeExecutionResult,
    },
    TransactionWithExecutionInfo, Uptime,
};

/// A type of the payload being returned in a binary response.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum PayloadType {
    /// Legacy version of the block header.
    BlockHeaderV1,
    /// Block header.
    BlockHeader,
    /// Legacy version of the block body.
    BlockBodyV1,
    /// Block body.
    BlockBody,
    /// Legacy version of the approvals hashes.
    ApprovalsHashesV1,
    /// Approvals hashes
    ApprovalsHashes,
    /// Block signatures.
    BlockSignatures,
    /// Deploy.
    Deploy,
    /// Transaction.
    Transaction,
    /// Legacy version of the execution result.
    ExecutionResultV1,
    /// Execution result.
    ExecutionResult,
    /// Transfers.
    Transfers,
    /// Finalized deploy approvals.
    FinalizedDeployApprovals,
    /// Finalized approvals.
    FinalizedApprovals,
    /// Block with signatures.
    SignedBlock,
    /// Transaction with approvals and execution info.
    TransactionWithExecutionInfo,
    /// Peers.
    Peers,
    /// Last progress.
    LastProgress,
    /// State of the reactor.
    ReactorState,
    /// Network name.
    NetworkName,
    /// Consensus validator changes.
    ConsensusValidatorChanges, // return type in `effects.rs` will be turned into dedicated type.
    /// Status of the block synchronizer.
    BlockSynchronizerStatus,
    /// Available block range.
    AvailableBlockRange,
    /// Information about the next network upgrade.
    NextUpgrade,
    /// Consensus status.
    ConsensusStatus, // return type in `effects.rs` will be turned into dedicated type.
    /// Chainspec represented as raw bytes.
    ChainspecRawBytes,
    /// Uptime.
    Uptime,
    /// Result of checking if given block is in the highest available block range.
    HighestBlockSequenceCheckResult,
    /// Result of the speculative execution,
    SpeculativeExecutionResult,
    /// Result of querying global state,
    GlobalStateQueryResult,
    /// Result of querying global state for all values under a specified key.
    StoredValues,
    /// Result of querying global state for a full trie.
    GetTrieFullResult,
    /// Node status.
    NodeStatus,
}

impl PayloadType {
    pub(crate) fn new_from_record_id(record_id: RecordId, is_legacy: bool) -> Self {
        match (is_legacy, record_id) {
            (true, RecordId::BlockHeader) => Self::BlockHeaderV1,
            (true, RecordId::BlockBody) => Self::BlockBodyV1,
            (true, RecordId::ApprovalsHashes) => Self::ApprovalsHashesV1,
            (true, RecordId::BlockMetadata) => Self::BlockSignatures,
            (true, RecordId::Transaction) => Self::Deploy,
            (true, RecordId::ExecutionResult) => Self::ExecutionResultV1,
            (true, RecordId::Transfer) => Self::Transfers,
            (true, RecordId::FinalizedTransactionApprovals) => Self::FinalizedDeployApprovals,
            (false, RecordId::BlockHeader) => Self::BlockHeader,
            (false, RecordId::BlockBody) => Self::BlockBody,
            (false, RecordId::ApprovalsHashes) => Self::ApprovalsHashes,
            (false, RecordId::BlockMetadata) => Self::BlockSignatures,
            (false, RecordId::Transaction) => Self::Transaction,
            (false, RecordId::ExecutionResult) => Self::ExecutionResult,
            (false, RecordId::Transfer) => Self::Transfers,
            (false, RecordId::FinalizedTransactionApprovals) => Self::FinalizedApprovals,
        }
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self::try_from(rng.gen_range(0..33)).unwrap()
    }
}

impl TryFrom<u8> for PayloadType {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            x if x == PayloadType::BlockHeaderV1 as u8 => Ok(PayloadType::BlockHeaderV1),
            x if x == PayloadType::BlockHeader as u8 => Ok(PayloadType::BlockHeader),
            x if x == PayloadType::BlockBodyV1 as u8 => Ok(PayloadType::BlockBodyV1),
            x if x == PayloadType::BlockBody as u8 => Ok(PayloadType::BlockBody),
            x if x == PayloadType::ApprovalsHashesV1 as u8 => Ok(PayloadType::ApprovalsHashesV1),
            x if x == PayloadType::ApprovalsHashes as u8 => Ok(PayloadType::ApprovalsHashes),
            x if x == PayloadType::BlockSignatures as u8 => Ok(PayloadType::BlockSignatures),
            x if x == PayloadType::Deploy as u8 => Ok(PayloadType::Deploy),
            x if x == PayloadType::Transaction as u8 => Ok(PayloadType::Transaction),
            x if x == PayloadType::ExecutionResultV1 as u8 => Ok(PayloadType::ExecutionResultV1),
            x if x == PayloadType::ExecutionResult as u8 => Ok(PayloadType::ExecutionResult),
            x if x == PayloadType::Transfers as u8 => Ok(PayloadType::Transfers),
            x if x == PayloadType::FinalizedDeployApprovals as u8 => {
                Ok(PayloadType::FinalizedDeployApprovals)
            }
            x if x == PayloadType::FinalizedApprovals as u8 => Ok(PayloadType::FinalizedApprovals),
            x if x == PayloadType::Peers as u8 => Ok(PayloadType::Peers),
            x if x == PayloadType::LastProgress as u8 => Ok(PayloadType::LastProgress),
            x if x == PayloadType::ReactorState as u8 => Ok(PayloadType::ReactorState),
            x if x == PayloadType::NetworkName as u8 => Ok(PayloadType::NetworkName),
            x if x == PayloadType::ConsensusValidatorChanges as u8 => {
                Ok(PayloadType::ConsensusValidatorChanges)
            }
            x if x == PayloadType::BlockSynchronizerStatus as u8 => {
                Ok(PayloadType::BlockSynchronizerStatus)
            }
            x if x == PayloadType::AvailableBlockRange as u8 => {
                Ok(PayloadType::AvailableBlockRange)
            }
            x if x == PayloadType::NextUpgrade as u8 => Ok(PayloadType::NextUpgrade),
            x if x == PayloadType::ConsensusStatus as u8 => Ok(PayloadType::ConsensusStatus),
            x if x == PayloadType::ChainspecRawBytes as u8 => Ok(PayloadType::ChainspecRawBytes),
            x if x == PayloadType::Uptime as u8 => Ok(PayloadType::Uptime),
            x if x == PayloadType::HighestBlockSequenceCheckResult as u8 => {
                Ok(PayloadType::HighestBlockSequenceCheckResult)
            }
            x if x == PayloadType::SpeculativeExecutionResult as u8 => {
                Ok(PayloadType::SpeculativeExecutionResult)
            }
            x if x == PayloadType::GlobalStateQueryResult as u8 => {
                Ok(PayloadType::GlobalStateQueryResult)
            }
            x if x == PayloadType::StoredValues as u8 => Ok(PayloadType::StoredValues),
            x if x == PayloadType::GetTrieFullResult as u8 => Ok(PayloadType::GetTrieFullResult),
            x if x == PayloadType::NodeStatus as u8 => Ok(PayloadType::NodeStatus),
            _ => Err(()),
        }
    }
}

impl From<PayloadType> for u8 {
    fn from(value: PayloadType) -> Self {
        value as u8
    }
}

impl fmt::Display for PayloadType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PayloadType::BlockHeaderV1 => write!(f, "BlockHeaderV1"),
            PayloadType::BlockHeader => write!(f, "BlockHeader"),
            PayloadType::BlockBodyV1 => write!(f, "BlockBodyV1"),
            PayloadType::BlockBody => write!(f, "BlockBody"),
            PayloadType::ApprovalsHashesV1 => write!(f, "ApprovalsHashesV1"),
            PayloadType::ApprovalsHashes => write!(f, "ApprovalsHashes"),
            PayloadType::BlockSignatures => write!(f, "BlockSignatures"),
            PayloadType::Deploy => write!(f, "Deploy"),
            PayloadType::Transaction => write!(f, "Transaction"),
            PayloadType::ExecutionResultV1 => write!(f, "ExecutionResultV1"),
            PayloadType::ExecutionResult => write!(f, "ExecutionResult"),
            PayloadType::Transfers => write!(f, "Transfers"),
            PayloadType::FinalizedDeployApprovals => write!(f, "FinalizedDeployApprovals"),
            PayloadType::FinalizedApprovals => write!(f, "FinalizedApprovals"),
            PayloadType::SignedBlock => write!(f, "SignedBlock"),
            PayloadType::TransactionWithExecutionInfo => write!(f, "TransactionWithExecutionInfo"),
            PayloadType::Peers => write!(f, "Peers"),
            PayloadType::LastProgress => write!(f, "LastProgress"),
            PayloadType::ReactorState => write!(f, "ReactorState"),
            PayloadType::NetworkName => write!(f, "NetworkName"),
            PayloadType::ConsensusValidatorChanges => write!(f, "ConsensusValidatorChanges"),
            PayloadType::BlockSynchronizerStatus => write!(f, "BlockSynchronizerStatus"),
            PayloadType::AvailableBlockRange => write!(f, "AvailableBlockRange"),
            PayloadType::NextUpgrade => write!(f, "NextUpgrade"),
            PayloadType::ConsensusStatus => write!(f, "ConsensusStatus"),
            PayloadType::ChainspecRawBytes => write!(f, "ChainspecRawBytes"),
            PayloadType::Uptime => write!(f, "Uptime"),
            PayloadType::HighestBlockSequenceCheckResult => {
                write!(f, "HighestBlockSequenceCheckResult")
            }
            PayloadType::SpeculativeExecutionResult => write!(f, "SpeculativeExecutionResult"),
            PayloadType::GlobalStateQueryResult => write!(f, "GlobalStateQueryResult"),
            PayloadType::StoredValues => write!(f, "StoredValues"),
            PayloadType::GetTrieFullResult => write!(f, "GetTrieFullResult"),
            PayloadType::NodeStatus => write!(f, "NodeStatus"),
        }
    }
}

const BLOCK_HEADER_V1_TAG: u8 = 0;
const BLOCK_HEADER_TAG: u8 = 1;
const BLOCK_BODY_V1_TAG: u8 = 2;
const BLOCK_BODY_TAG: u8 = 3;
const APPROVALS_HASHES_TAG: u8 = 4;
const APPROVALS_HASHES_V1: u8 = 5;
const BLOCK_SIGNATURES_TAG: u8 = 6;
const DEPLOY_TAG: u8 = 7;
const TRANSACTION_TAG: u8 = 8;
const EXECUTION_RESULT_V1_TAG: u8 = 9;
const EXECUTION_RESULT_TAG: u8 = 10;
const TRANSFERS_TAG: u8 = 11;
const FINALIZED_DEPLOY_APPROVALS_TAG: u8 = 12;
const FINALIZED_APPROVALS_TAG: u8 = 13;
const SIGNED_BLOCK_TAG: u8 = 14;
const TRANSACTION_WITH_EXECUTION_INFO_TAG: u8 = 15;
const PEERS_TAG: u8 = 16;
const UPTIME_TAG: u8 = 17;
const LAST_PROGRESS_TAG: u8 = 18;
const REACTOR_STATE_TAG: u8 = 19;
const NETWORK_NAME_TAG: u8 = 20;
const CONSENSUS_VALIDATOR_CHANGES_TAG: u8 = 21;
const BLOCK_SYNCHRONIZER_STATUS_TAG: u8 = 22;
const AVAILABLE_BLOCK_RANGE_TAG: u8 = 23;
const NEXT_UPGRADE_TAG: u8 = 24;
const CONSENSUS_STATUS_TAG: u8 = 25;
const CHAINSPEC_RAW_BYTES_TAG: u8 = 26;
const HIGHEST_BLOCK_SEQUENCE_CHECK_RESULT_TAG: u8 = 27;
const SPECULATIVE_EXECUTION_RESULT_TAG: u8 = 28;
const GLOBAL_STATE_QUERY_RESULT_TAG: u8 = 29;
const STORED_VALUES_TAG: u8 = 30;
const GET_TRIE_FULL_RESULT_TAG: u8 = 31;
const NODE_STATUS_TAG: u8 = 32;

impl ToBytes for PayloadType {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PayloadType::BlockHeaderV1 => BLOCK_HEADER_V1_TAG,
            PayloadType::BlockHeader => BLOCK_HEADER_TAG,
            PayloadType::BlockBodyV1 => BLOCK_BODY_V1_TAG,
            PayloadType::BlockBody => BLOCK_BODY_TAG,
            PayloadType::ApprovalsHashes => APPROVALS_HASHES_TAG,
            PayloadType::ApprovalsHashesV1 => APPROVALS_HASHES_V1,
            PayloadType::BlockSignatures => BLOCK_SIGNATURES_TAG,
            PayloadType::Deploy => DEPLOY_TAG,
            PayloadType::Transaction => TRANSACTION_TAG,
            PayloadType::ExecutionResultV1 => EXECUTION_RESULT_V1_TAG,
            PayloadType::ExecutionResult => EXECUTION_RESULT_TAG,
            PayloadType::Transfers => TRANSFERS_TAG,
            PayloadType::FinalizedDeployApprovals => FINALIZED_DEPLOY_APPROVALS_TAG,
            PayloadType::FinalizedApprovals => FINALIZED_APPROVALS_TAG,
            PayloadType::Peers => PEERS_TAG,
            PayloadType::SignedBlock => SIGNED_BLOCK_TAG,
            PayloadType::TransactionWithExecutionInfo => TRANSACTION_WITH_EXECUTION_INFO_TAG,
            PayloadType::LastProgress => LAST_PROGRESS_TAG,
            PayloadType::ReactorState => REACTOR_STATE_TAG,
            PayloadType::NetworkName => NETWORK_NAME_TAG,
            PayloadType::ConsensusValidatorChanges => CONSENSUS_VALIDATOR_CHANGES_TAG,
            PayloadType::BlockSynchronizerStatus => BLOCK_SYNCHRONIZER_STATUS_TAG,
            PayloadType::AvailableBlockRange => AVAILABLE_BLOCK_RANGE_TAG,
            PayloadType::NextUpgrade => NEXT_UPGRADE_TAG,
            PayloadType::ConsensusStatus => CONSENSUS_STATUS_TAG,
            PayloadType::ChainspecRawBytes => CHAINSPEC_RAW_BYTES_TAG,
            PayloadType::Uptime => UPTIME_TAG,
            PayloadType::HighestBlockSequenceCheckResult => HIGHEST_BLOCK_SEQUENCE_CHECK_RESULT_TAG,
            PayloadType::SpeculativeExecutionResult => SPECULATIVE_EXECUTION_RESULT_TAG,
            PayloadType::GlobalStateQueryResult => GLOBAL_STATE_QUERY_RESULT_TAG,
            PayloadType::StoredValues => STORED_VALUES_TAG,
            PayloadType::GetTrieFullResult => GET_TRIE_FULL_RESULT_TAG,
            PayloadType::NodeStatus => NODE_STATUS_TAG,
        }
        .write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for PayloadType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = FromBytes::from_bytes(bytes)?;
        let record_id = match tag {
            BLOCK_HEADER_V1_TAG => PayloadType::BlockHeaderV1,
            BLOCK_HEADER_TAG => PayloadType::BlockHeader,
            BLOCK_BODY_V1_TAG => PayloadType::BlockBodyV1,
            BLOCK_BODY_TAG => PayloadType::BlockBody,
            APPROVALS_HASHES_TAG => PayloadType::ApprovalsHashes,
            APPROVALS_HASHES_V1 => PayloadType::ApprovalsHashesV1,
            BLOCK_SIGNATURES_TAG => PayloadType::BlockSignatures,
            DEPLOY_TAG => PayloadType::Deploy,
            TRANSACTION_TAG => PayloadType::Transaction,
            EXECUTION_RESULT_V1_TAG => PayloadType::ExecutionResultV1,
            EXECUTION_RESULT_TAG => PayloadType::ExecutionResult,
            TRANSFERS_TAG => PayloadType::Transfers,
            FINALIZED_DEPLOY_APPROVALS_TAG => PayloadType::FinalizedDeployApprovals,
            FINALIZED_APPROVALS_TAG => PayloadType::FinalizedApprovals,
            PEERS_TAG => PayloadType::Peers,
            SIGNED_BLOCK_TAG => PayloadType::SignedBlock,
            TRANSACTION_WITH_EXECUTION_INFO_TAG => PayloadType::TransactionWithExecutionInfo,
            LAST_PROGRESS_TAG => PayloadType::LastProgress,
            REACTOR_STATE_TAG => PayloadType::ReactorState,
            NETWORK_NAME_TAG => PayloadType::NetworkName,
            CONSENSUS_VALIDATOR_CHANGES_TAG => PayloadType::ConsensusValidatorChanges,
            BLOCK_SYNCHRONIZER_STATUS_TAG => PayloadType::BlockSynchronizerStatus,
            AVAILABLE_BLOCK_RANGE_TAG => PayloadType::AvailableBlockRange,
            NEXT_UPGRADE_TAG => PayloadType::NextUpgrade,
            CONSENSUS_STATUS_TAG => PayloadType::ConsensusStatus,
            CHAINSPEC_RAW_BYTES_TAG => PayloadType::ChainspecRawBytes,
            UPTIME_TAG => PayloadType::Uptime,
            HIGHEST_BLOCK_SEQUENCE_CHECK_RESULT_TAG => PayloadType::HighestBlockSequenceCheckResult,
            SPECULATIVE_EXECUTION_RESULT_TAG => PayloadType::SpeculativeExecutionResult,
            GLOBAL_STATE_QUERY_RESULT_TAG => PayloadType::GlobalStateQueryResult,
            STORED_VALUES_TAG => PayloadType::StoredValues,
            GET_TRIE_FULL_RESULT_TAG => PayloadType::GetTrieFullResult,
            NODE_STATUS_TAG => PayloadType::NodeStatus,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((record_id, remainder))
    }
}

/// Represents an entity that can be sent as a payload.
pub trait PayloadEntity {
    /// Returns the payload type of the entity.
    const PAYLOAD_TYPE: PayloadType;
}

impl PayloadEntity for Transaction {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Transaction;
}

impl PayloadEntity for Deploy {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Deploy;
}

impl PayloadEntity for BlockHeader {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockHeader;
}

impl PayloadEntity for BlockHeaderV1 {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockHeaderV1;
}

impl PayloadEntity for BlockBody {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockBody;
}

impl PayloadEntity for BlockBodyV1 {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockBodyV1;
}

impl PayloadEntity for ExecutionResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ExecutionResult;
}

impl PayloadEntity for FinalizedApprovals {
    const PAYLOAD_TYPE: PayloadType = PayloadType::FinalizedApprovals;
}

impl PayloadEntity for FinalizedDeployApprovals {
    const PAYLOAD_TYPE: PayloadType = PayloadType::FinalizedDeployApprovals;
}

impl PayloadEntity for ExecutionResultV1 {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ExecutionResultV1;
}

impl PayloadEntity for SignedBlock {
    const PAYLOAD_TYPE: PayloadType = PayloadType::SignedBlock;
}

impl PayloadEntity for TransactionWithExecutionInfo {
    const PAYLOAD_TYPE: PayloadType = PayloadType::TransactionWithExecutionInfo;
}

impl PayloadEntity for Peers {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Peers;
}

impl PayloadEntity for BlockSignatures {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockSignatures;
}

impl PayloadEntity for Vec<Transfer> {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Transfers;
}

impl PayloadEntity for AvailableBlockRange {
    const PAYLOAD_TYPE: PayloadType = PayloadType::AvailableBlockRange;
}

#[cfg(any(feature = "std", test))]
impl PayloadEntity for ChainspecRawBytes {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ChainspecRawBytes;
}

impl PayloadEntity for ConsensusValidatorChanges {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ConsensusValidatorChanges;
}

impl PayloadEntity for GlobalStateQueryResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::GlobalStateQueryResult;
}

impl PayloadEntity for Vec<StoredValue> {
    const PAYLOAD_TYPE: PayloadType = PayloadType::StoredValues;
}

impl PayloadEntity for GetTrieFullResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::GetTrieFullResult;
}

impl PayloadEntity for SpeculativeExecutionResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::SpeculativeExecutionResult;
}

#[cfg(any(feature = "std", test))]
impl PayloadEntity for NodeStatus {
    const PAYLOAD_TYPE: PayloadType = PayloadType::NodeStatus;
}

#[cfg(any(feature = "std", test))]
impl PayloadEntity for NextUpgrade {
    const PAYLOAD_TYPE: PayloadType = PayloadType::NextUpgrade;
}

impl PayloadEntity for Uptime {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Uptime;
}

impl PayloadEntity for LastProgress {
    const PAYLOAD_TYPE: PayloadType = PayloadType::LastProgress;
}

impl PayloadEntity for ReactorState {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ReactorState;
}

impl PayloadEntity for NetworkName {
    const PAYLOAD_TYPE: PayloadType = PayloadType::NetworkName;
}

impl PayloadEntity for BlockSynchronizerStatus {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockSynchronizerStatus;
}

impl PayloadEntity for ConsensusStatus {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ConsensusStatus;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = PayloadType::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
