//! The payload type.

use core::{convert::TryFrom, fmt};

#[cfg(test)]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    execution::{ExecutionResult, ExecutionResultV1},
    AvailableBlockRange, BlockBody, BlockBodyV1, BlockHeader, BlockHeaderV1, BlockSignatures,
    BlockSignaturesV1, BlockSynchronizerStatus, ChainspecRawBytes, Deploy, NextUpgrade, Peers,
    SignedBlock, StoredValue, Transaction, Transfer,
};

use crate::{
    global_state_query_result::GlobalStateQueryResult,
    node_status::NodeStatus,
    speculative_execution_result::SpeculativeExecutionResult,
    type_wrappers::{
        ConsensusStatus, ConsensusValidatorChanges, GetTrieFullResult, LastProgress, NetworkName,
        ReactorStateName, Reward,
    },
    BalanceResponse, DictionaryQueryResult, RecordId, TransactionWithExecutionInfo, Uptime,
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
    /// Legacy version of the block signatures.
    BlockSignaturesV1,
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
    /// Wasm V1 execution result.
    WasmV1Result,
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
    /// Result of querying for a dictionary item.
    DictionaryQueryResult,
    /// Balance query response.
    BalanceResponse,
    /// Reward response.
    Reward,
}

impl PayloadType {
    pub fn from_record_id(record_id: RecordId, is_legacy: bool) -> Self {
        match (is_legacy, record_id) {
            (true, RecordId::BlockHeader) => Self::BlockHeaderV1,
            (true, RecordId::BlockBody) => Self::BlockBodyV1,
            (true, RecordId::ApprovalsHashes) => Self::ApprovalsHashesV1,
            (true, RecordId::BlockMetadata) => Self::BlockSignaturesV1,
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
        Self::try_from(rng.gen_range(0..38)).unwrap()
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
            x if x == PayloadType::BlockSignaturesV1 as u8 => Ok(PayloadType::BlockSignaturesV1),
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
            x if x == PayloadType::SignedBlock as u8 => Ok(PayloadType::SignedBlock),
            x if x == PayloadType::TransactionWithExecutionInfo as u8 => {
                Ok(PayloadType::TransactionWithExecutionInfo)
            }
            x if x == PayloadType::Peers as u8 => Ok(PayloadType::Peers),
            x if x == PayloadType::Uptime as u8 => Ok(PayloadType::Uptime),
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
            x if x == PayloadType::DictionaryQueryResult as u8 => {
                Ok(PayloadType::DictionaryQueryResult)
            }
            x if x == PayloadType::WasmV1Result as u8 => Ok(PayloadType::WasmV1Result),
            x if x == PayloadType::BalanceResponse as u8 => Ok(PayloadType::BalanceResponse),
            x if x == PayloadType::Reward as u8 => Ok(PayloadType::Reward),
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
            PayloadType::BlockSignaturesV1 => write!(f, "BlockSignaturesV1"),
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
            PayloadType::WasmV1Result => write!(f, "WasmV1Result"),
            PayloadType::DictionaryQueryResult => write!(f, "DictionaryQueryResult"),
            PayloadType::BalanceResponse => write!(f, "BalanceResponse"),
            PayloadType::Reward => write!(f, "Reward"),
        }
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

impl PayloadEntity for BlockSignatures {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockSignatures;
}

impl PayloadEntity for BlockSignaturesV1 {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockSignaturesV1;
}

impl PayloadEntity for ExecutionResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ExecutionResult;
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

impl PayloadEntity for Vec<Transfer> {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Transfers;
}

impl PayloadEntity for AvailableBlockRange {
    const PAYLOAD_TYPE: PayloadType = PayloadType::AvailableBlockRange;
}

impl PayloadEntity for ChainspecRawBytes {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ChainspecRawBytes;
}

impl PayloadEntity for ConsensusValidatorChanges {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ConsensusValidatorChanges;
}

impl PayloadEntity for GlobalStateQueryResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::GlobalStateQueryResult;
}

impl PayloadEntity for DictionaryQueryResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::DictionaryQueryResult;
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

impl PayloadEntity for NodeStatus {
    const PAYLOAD_TYPE: PayloadType = PayloadType::NodeStatus;
}

impl PayloadEntity for NextUpgrade {
    const PAYLOAD_TYPE: PayloadType = PayloadType::NextUpgrade;
}

impl PayloadEntity for Uptime {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Uptime;
}

impl PayloadEntity for LastProgress {
    const PAYLOAD_TYPE: PayloadType = PayloadType::LastProgress;
}

impl PayloadEntity for ReactorStateName {
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

impl PayloadEntity for BalanceResponse {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BalanceResponse;
}

impl PayloadEntity for Reward {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Reward;
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn convert_u8_roundtrip() {
        let rng = &mut TestRng::new();

        let val = PayloadType::random(rng);
        assert_eq!(PayloadType::try_from(val as u8), Ok(val));
    }
}
