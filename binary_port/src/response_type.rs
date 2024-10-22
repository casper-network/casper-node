//! The payload type.

use core::{convert::TryFrom, fmt};

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    contracts::ContractPackage,
    execution::{ExecutionResult, ExecutionResultV1},
    AvailableBlockRange, BlockBody, BlockBodyV1, BlockHeader, BlockHeaderV1, BlockSignatures,
    BlockSignaturesV1, BlockSynchronizerStatus, ChainspecRawBytes, Deploy, NextUpgrade, Package,
    Peers, ProtocolVersion, SignedBlock, StoredValue, Transaction, Transfer,
};

use crate::{
    global_state_query_result::GlobalStateQueryResult,
    node_status::NodeStatus,
    speculative_execution_result::SpeculativeExecutionResult,
    type_wrappers::{
        ConsensusStatus, ConsensusValidatorChanges, GetTrieFullResult, LastProgress, NetworkName,
        ReactorStateName, RewardResponse,
    },
    AccountInformation, AddressableEntityInformation, BalanceResponse, ContractInformation,
    DictionaryQueryResult, RecordId, TransactionWithExecutionInfo, Uptime, ValueWithProof,
};

/// A type of the payload being returned in a binary response.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum ResponseType {
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
    /// Protocol version.
    ProtocolVersion,
    /// Contract package with Merkle proof.
    ContractPackageWithProof,
    /// Contract information.
    ContractInformation,
    /// Account information.
    AccountInformation,
    /// Package with Merkle proof.
    PackageWithProof,
    /// Addressable entity information.
    AddressableEntityInformation,
}

impl ResponseType {
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
        Self::try_from(rng.gen_range(0..44)).unwrap()
    }
}

impl TryFrom<u8> for ResponseType {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            x if x == ResponseType::BlockHeaderV1 as u8 => Ok(ResponseType::BlockHeaderV1),
            x if x == ResponseType::BlockHeader as u8 => Ok(ResponseType::BlockHeader),
            x if x == ResponseType::BlockBodyV1 as u8 => Ok(ResponseType::BlockBodyV1),
            x if x == ResponseType::BlockBody as u8 => Ok(ResponseType::BlockBody),
            x if x == ResponseType::ApprovalsHashesV1 as u8 => Ok(ResponseType::ApprovalsHashesV1),
            x if x == ResponseType::ApprovalsHashes as u8 => Ok(ResponseType::ApprovalsHashes),
            x if x == ResponseType::BlockSignaturesV1 as u8 => Ok(ResponseType::BlockSignaturesV1),
            x if x == ResponseType::BlockSignatures as u8 => Ok(ResponseType::BlockSignatures),
            x if x == ResponseType::Deploy as u8 => Ok(ResponseType::Deploy),
            x if x == ResponseType::Transaction as u8 => Ok(ResponseType::Transaction),
            x if x == ResponseType::ExecutionResultV1 as u8 => Ok(ResponseType::ExecutionResultV1),
            x if x == ResponseType::ExecutionResult as u8 => Ok(ResponseType::ExecutionResult),
            x if x == ResponseType::Transfers as u8 => Ok(ResponseType::Transfers),
            x if x == ResponseType::FinalizedDeployApprovals as u8 => {
                Ok(ResponseType::FinalizedDeployApprovals)
            }
            x if x == ResponseType::FinalizedApprovals as u8 => {
                Ok(ResponseType::FinalizedApprovals)
            }
            x if x == ResponseType::SignedBlock as u8 => Ok(ResponseType::SignedBlock),
            x if x == ResponseType::TransactionWithExecutionInfo as u8 => {
                Ok(ResponseType::TransactionWithExecutionInfo)
            }
            x if x == ResponseType::Peers as u8 => Ok(ResponseType::Peers),
            x if x == ResponseType::Uptime as u8 => Ok(ResponseType::Uptime),
            x if x == ResponseType::LastProgress as u8 => Ok(ResponseType::LastProgress),
            x if x == ResponseType::ReactorState as u8 => Ok(ResponseType::ReactorState),
            x if x == ResponseType::NetworkName as u8 => Ok(ResponseType::NetworkName),
            x if x == ResponseType::ConsensusValidatorChanges as u8 => {
                Ok(ResponseType::ConsensusValidatorChanges)
            }
            x if x == ResponseType::BlockSynchronizerStatus as u8 => {
                Ok(ResponseType::BlockSynchronizerStatus)
            }
            x if x == ResponseType::AvailableBlockRange as u8 => {
                Ok(ResponseType::AvailableBlockRange)
            }
            x if x == ResponseType::NextUpgrade as u8 => Ok(ResponseType::NextUpgrade),
            x if x == ResponseType::ConsensusStatus as u8 => Ok(ResponseType::ConsensusStatus),
            x if x == ResponseType::ChainspecRawBytes as u8 => Ok(ResponseType::ChainspecRawBytes),
            x if x == ResponseType::HighestBlockSequenceCheckResult as u8 => {
                Ok(ResponseType::HighestBlockSequenceCheckResult)
            }
            x if x == ResponseType::SpeculativeExecutionResult as u8 => {
                Ok(ResponseType::SpeculativeExecutionResult)
            }
            x if x == ResponseType::GlobalStateQueryResult as u8 => {
                Ok(ResponseType::GlobalStateQueryResult)
            }
            x if x == ResponseType::StoredValues as u8 => Ok(ResponseType::StoredValues),
            x if x == ResponseType::GetTrieFullResult as u8 => Ok(ResponseType::GetTrieFullResult),
            x if x == ResponseType::NodeStatus as u8 => Ok(ResponseType::NodeStatus),
            x if x == ResponseType::DictionaryQueryResult as u8 => {
                Ok(ResponseType::DictionaryQueryResult)
            }
            x if x == ResponseType::WasmV1Result as u8 => Ok(ResponseType::WasmV1Result),
            x if x == ResponseType::BalanceResponse as u8 => Ok(ResponseType::BalanceResponse),
            x if x == ResponseType::Reward as u8 => Ok(ResponseType::Reward),
            x if x == ResponseType::ProtocolVersion as u8 => Ok(ResponseType::ProtocolVersion),
            x if x == ResponseType::ContractPackageWithProof as u8 => {
                Ok(ResponseType::ContractPackageWithProof)
            }
            x if x == ResponseType::ContractInformation as u8 => {
                Ok(ResponseType::ContractInformation)
            }
            x if x == ResponseType::AccountInformation as u8 => {
                Ok(ResponseType::AccountInformation)
            }
            x if x == ResponseType::PackageWithProof as u8 => Ok(ResponseType::PackageWithProof),
            x if x == ResponseType::AddressableEntityInformation as u8 => {
                Ok(ResponseType::AddressableEntityInformation)
            }
            _ => Err(()),
        }
    }
}

impl From<ResponseType> for u8 {
    fn from(value: ResponseType) -> Self {
        value as u8
    }
}

impl fmt::Display for ResponseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResponseType::BlockHeaderV1 => write!(f, "BlockHeaderV1"),
            ResponseType::BlockHeader => write!(f, "BlockHeader"),
            ResponseType::BlockBodyV1 => write!(f, "BlockBodyV1"),
            ResponseType::BlockBody => write!(f, "BlockBody"),
            ResponseType::ApprovalsHashesV1 => write!(f, "ApprovalsHashesV1"),
            ResponseType::ApprovalsHashes => write!(f, "ApprovalsHashes"),
            ResponseType::BlockSignaturesV1 => write!(f, "BlockSignaturesV1"),
            ResponseType::BlockSignatures => write!(f, "BlockSignatures"),
            ResponseType::Deploy => write!(f, "Deploy"),
            ResponseType::Transaction => write!(f, "Transaction"),
            ResponseType::ExecutionResultV1 => write!(f, "ExecutionResultV1"),
            ResponseType::ExecutionResult => write!(f, "ExecutionResult"),
            ResponseType::Transfers => write!(f, "Transfers"),
            ResponseType::FinalizedDeployApprovals => write!(f, "FinalizedDeployApprovals"),
            ResponseType::FinalizedApprovals => write!(f, "FinalizedApprovals"),
            ResponseType::SignedBlock => write!(f, "SignedBlock"),
            ResponseType::TransactionWithExecutionInfo => write!(f, "TransactionWithExecutionInfo"),
            ResponseType::Peers => write!(f, "Peers"),
            ResponseType::LastProgress => write!(f, "LastProgress"),
            ResponseType::ReactorState => write!(f, "ReactorState"),
            ResponseType::NetworkName => write!(f, "NetworkName"),
            ResponseType::ConsensusValidatorChanges => write!(f, "ConsensusValidatorChanges"),
            ResponseType::BlockSynchronizerStatus => write!(f, "BlockSynchronizerStatus"),
            ResponseType::AvailableBlockRange => write!(f, "AvailableBlockRange"),
            ResponseType::NextUpgrade => write!(f, "NextUpgrade"),
            ResponseType::ConsensusStatus => write!(f, "ConsensusStatus"),
            ResponseType::ChainspecRawBytes => write!(f, "ChainspecRawBytes"),
            ResponseType::Uptime => write!(f, "Uptime"),
            ResponseType::HighestBlockSequenceCheckResult => {
                write!(f, "HighestBlockSequenceCheckResult")
            }
            ResponseType::SpeculativeExecutionResult => write!(f, "SpeculativeExecutionResult"),
            ResponseType::GlobalStateQueryResult => write!(f, "GlobalStateQueryResult"),
            ResponseType::StoredValues => write!(f, "StoredValues"),
            ResponseType::GetTrieFullResult => write!(f, "GetTrieFullResult"),
            ResponseType::NodeStatus => write!(f, "NodeStatus"),
            ResponseType::WasmV1Result => write!(f, "WasmV1Result"),
            ResponseType::DictionaryQueryResult => write!(f, "DictionaryQueryResult"),
            ResponseType::BalanceResponse => write!(f, "BalanceResponse"),
            ResponseType::Reward => write!(f, "Reward"),
            ResponseType::ProtocolVersion => write!(f, "ProtocolVersion"),
            ResponseType::ContractPackageWithProof => write!(f, "ContractPackageWithProof"),
            ResponseType::ContractInformation => write!(f, "ContractInformation"),
            ResponseType::AccountInformation => write!(f, "AccountInformation"),
            ResponseType::PackageWithProof => write!(f, "PackageWithProof"),
            ResponseType::AddressableEntityInformation => {
                write!(f, "AddressableEntityInformation")
            }
        }
    }
}

/// Represents an entity that can be sent as a payload.
pub trait PayloadEntity {
    /// Returns the payload type of the entity.
    const RESPONSE_TYPE: ResponseType;
}

impl PayloadEntity for Transaction {
    const RESPONSE_TYPE: ResponseType = ResponseType::Transaction;
}

impl PayloadEntity for Deploy {
    const RESPONSE_TYPE: ResponseType = ResponseType::Deploy;
}

impl PayloadEntity for BlockHeader {
    const RESPONSE_TYPE: ResponseType = ResponseType::BlockHeader;
}

impl PayloadEntity for BlockHeaderV1 {
    const RESPONSE_TYPE: ResponseType = ResponseType::BlockHeaderV1;
}

impl PayloadEntity for BlockBody {
    const RESPONSE_TYPE: ResponseType = ResponseType::BlockBody;
}

impl PayloadEntity for BlockBodyV1 {
    const RESPONSE_TYPE: ResponseType = ResponseType::BlockBodyV1;
}

impl PayloadEntity for BlockSignatures {
    const RESPONSE_TYPE: ResponseType = ResponseType::BlockSignatures;
}

impl PayloadEntity for BlockSignaturesV1 {
    const RESPONSE_TYPE: ResponseType = ResponseType::BlockSignaturesV1;
}

impl PayloadEntity for ExecutionResult {
    const RESPONSE_TYPE: ResponseType = ResponseType::ExecutionResult;
}

impl PayloadEntity for ExecutionResultV1 {
    const RESPONSE_TYPE: ResponseType = ResponseType::ExecutionResultV1;
}

impl PayloadEntity for SignedBlock {
    const RESPONSE_TYPE: ResponseType = ResponseType::SignedBlock;
}

impl PayloadEntity for TransactionWithExecutionInfo {
    const RESPONSE_TYPE: ResponseType = ResponseType::TransactionWithExecutionInfo;
}

impl PayloadEntity for Peers {
    const RESPONSE_TYPE: ResponseType = ResponseType::Peers;
}

impl PayloadEntity for Vec<Transfer> {
    const RESPONSE_TYPE: ResponseType = ResponseType::Transfers;
}

impl PayloadEntity for AvailableBlockRange {
    const RESPONSE_TYPE: ResponseType = ResponseType::AvailableBlockRange;
}

impl PayloadEntity for ChainspecRawBytes {
    const RESPONSE_TYPE: ResponseType = ResponseType::ChainspecRawBytes;
}

impl PayloadEntity for ConsensusValidatorChanges {
    const RESPONSE_TYPE: ResponseType = ResponseType::ConsensusValidatorChanges;
}

impl PayloadEntity for GlobalStateQueryResult {
    const RESPONSE_TYPE: ResponseType = ResponseType::GlobalStateQueryResult;
}

impl PayloadEntity for DictionaryQueryResult {
    const RESPONSE_TYPE: ResponseType = ResponseType::DictionaryQueryResult;
}

impl PayloadEntity for Vec<StoredValue> {
    const RESPONSE_TYPE: ResponseType = ResponseType::StoredValues;
}

impl PayloadEntity for GetTrieFullResult {
    const RESPONSE_TYPE: ResponseType = ResponseType::GetTrieFullResult;
}

impl PayloadEntity for SpeculativeExecutionResult {
    const RESPONSE_TYPE: ResponseType = ResponseType::SpeculativeExecutionResult;
}

impl PayloadEntity for NodeStatus {
    const RESPONSE_TYPE: ResponseType = ResponseType::NodeStatus;
}

impl PayloadEntity for NextUpgrade {
    const RESPONSE_TYPE: ResponseType = ResponseType::NextUpgrade;
}

impl PayloadEntity for Uptime {
    const RESPONSE_TYPE: ResponseType = ResponseType::Uptime;
}

impl PayloadEntity for LastProgress {
    const RESPONSE_TYPE: ResponseType = ResponseType::LastProgress;
}

impl PayloadEntity for ReactorStateName {
    const RESPONSE_TYPE: ResponseType = ResponseType::ReactorState;
}

impl PayloadEntity for NetworkName {
    const RESPONSE_TYPE: ResponseType = ResponseType::NetworkName;
}

impl PayloadEntity for BlockSynchronizerStatus {
    const RESPONSE_TYPE: ResponseType = ResponseType::BlockSynchronizerStatus;
}

impl PayloadEntity for ConsensusStatus {
    const RESPONSE_TYPE: ResponseType = ResponseType::ConsensusStatus;
}

impl PayloadEntity for BalanceResponse {
    const RESPONSE_TYPE: ResponseType = ResponseType::BalanceResponse;
}

impl PayloadEntity for RewardResponse {
    const RESPONSE_TYPE: ResponseType = ResponseType::Reward;
}

impl PayloadEntity for ProtocolVersion {
    const RESPONSE_TYPE: ResponseType = ResponseType::ProtocolVersion;
}

impl PayloadEntity for ValueWithProof<ContractPackage> {
    const RESPONSE_TYPE: ResponseType = ResponseType::ContractPackageWithProof;
}

impl PayloadEntity for ContractInformation {
    const RESPONSE_TYPE: ResponseType = ResponseType::ContractInformation;
}

impl PayloadEntity for AccountInformation {
    const RESPONSE_TYPE: ResponseType = ResponseType::AccountInformation;
}

impl PayloadEntity for ValueWithProof<Package> {
    const RESPONSE_TYPE: ResponseType = ResponseType::PackageWithProof;
}

impl PayloadEntity for AddressableEntityInformation {
    const RESPONSE_TYPE: ResponseType = ResponseType::AddressableEntityInformation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn convert_u8_roundtrip() {
        let rng = &mut TestRng::new();

        let val = ResponseType::random(rng);
        assert_eq!(ResponseType::try_from(val as u8), Ok(val));
    }
}
