//! The payload type.

#[cfg(feature = "json-schema")]
use schemars::JsonSchema;

#[cfg(test)]
use rand::Rng;

use alloc::vec::Vec;
use core::fmt;
use std::convert::TryFrom;

#[cfg(test)]
use crate::testing::TestRng;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    AvailableBlockRange, BlockHash, BlockHashAndHeight, BlockSynchronizerStatus, Peers, PublicKey,
    ReactorState, TimeDiff, Uptime,
};

use super::{
    db_id::DbId,
    global_state::GlobalStateQueryResult,
    type_wrappers::{
        ConsensusValidatorChanges, GetTrieFullResult, HighestBlockSequenceCheckResult,
        LastProgress, NetworkName, SpeculativeExecutionResult, StoredValues,
    },
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
    ApprovalsHashes, // TODO[RC]: not existing yet
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
    VecTransfers,
    /// Global state bytes.
    // TODO[RC]: Needs wrapper.
    VecU8,
    /// Finalized deploy approvals.
    FinalizedDeployApprovals,
    /// Finalized approvals.
    FinalizedApprovals,
    /// Block hash and height.
    BlockHashAndHeight,
    /// Block hash.
    BlockHash,
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
    pub(crate) fn new_from_db_id(db_id: &DbId, is_legacy: bool) -> Self {
        match (is_legacy, db_id) {
            (true, DbId::BlockHeader) => Self::BlockHeaderV1,
            (true, DbId::BlockBody) => Self::BlockBodyV1,
            (true, DbId::ApprovalsHashes) => Self::ApprovalsHashes,
            (true, DbId::BlockMetadata) => Self::BlockSignatures,
            (true, DbId::Transaction) => Self::Deploy,
            (true, DbId::ExecutionResult) => Self::ExecutionResultV1,
            (true, DbId::Transfer) => Self::VecTransfers,
            (true, DbId::StateStore) => Self::VecU8,
            (true, DbId::FinalizedTransactionApprovals) => Self::FinalizedDeployApprovals,
            (false, DbId::BlockHeader) => Self::BlockHeader,
            (false, DbId::BlockBody) => Self::BlockBody,
            (false, DbId::ApprovalsHashes) => Self::ApprovalsHashesV1,
            (false, DbId::BlockMetadata) => Self::BlockSignatures,
            (false, DbId::Transaction) => Self::Transaction,
            (false, DbId::ExecutionResult) => Self::ExecutionResult,
            (false, DbId::Transfer) => Self::VecTransfers,
            (false, DbId::StateStore) => Self::VecU8,
            (false, DbId::FinalizedTransactionApprovals) => Self::FinalizedApprovals,
        }
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self::try_from(rng.gen_range(0..33)).unwrap()
    }
}

impl TryFrom<u8> for PayloadType {
    type Error = ();

    // TODO: replace with macro or find better option
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
            x if x == PayloadType::VecTransfers as u8 => Ok(PayloadType::VecTransfers),
            x if x == PayloadType::VecU8 as u8 => Ok(PayloadType::VecU8),
            x if x == PayloadType::FinalizedDeployApprovals as u8 => {
                Ok(PayloadType::FinalizedDeployApprovals)
            }
            x if x == PayloadType::FinalizedApprovals as u8 => Ok(PayloadType::FinalizedApprovals),
            x if x == PayloadType::BlockHashAndHeight as u8 => Ok(PayloadType::BlockHashAndHeight),
            x if x == PayloadType::BlockHash as u8 => Ok(PayloadType::BlockHash),
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
            _ => Err(()),
        }
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
            PayloadType::VecTransfers => write!(f, "VecTransfers"),
            PayloadType::VecU8 => write!(f, "VecU8"),
            PayloadType::FinalizedDeployApprovals => write!(f, "FinalizedDeployApprovals"),
            PayloadType::FinalizedApprovals => write!(f, "FinalizedApprovals"),
            PayloadType::BlockHashAndHeight => write!(f, "BlockHashAndHeight"),
            PayloadType::BlockHash => write!(f, "BlockHash"),
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

impl<T> From<Option<T>> for PayloadType {
    fn from(_: Option<T>) -> Self {
        panic!("could this be a compile time error?");
    }
}

impl From<BlockHashAndHeight> for PayloadType {
    fn from(_: BlockHashAndHeight) -> Self {
        Self::BlockHashAndHeight
    }
}

impl From<HighestBlockSequenceCheckResult> for PayloadType {
    fn from(_: HighestBlockSequenceCheckResult) -> Self {
        Self::HighestBlockSequenceCheckResult
    }
}

impl From<BlockHash> for PayloadType {
    fn from(_: BlockHash) -> Self {
        Self::BlockHash
    }
}

impl From<Peers> for PayloadType {
    fn from(_: Peers) -> Self {
        Self::Peers
    }
}

impl From<NetworkName> for PayloadType {
    fn from(_: NetworkName) -> Self {
        Self::NetworkName
    }
}

impl From<ReactorState> for PayloadType {
    fn from(_: ReactorState) -> Self {
        Self::ReactorState
    }
}

impl From<BlockSynchronizerStatus> for PayloadType {
    fn from(_: BlockSynchronizerStatus) -> Self {
        Self::BlockSynchronizerStatus
    }
}

impl From<AvailableBlockRange> for PayloadType {
    fn from(_: AvailableBlockRange) -> Self {
        Self::AvailableBlockRange
    }
}

impl From<ConsensusValidatorChanges> for PayloadType {
    fn from(_: ConsensusValidatorChanges) -> Self {
        Self::ConsensusValidatorChanges
    }
}

#[cfg(any(feature = "std", test))]
impl From<crate::NextUpgrade> for PayloadType {
    fn from(_: crate::NextUpgrade) -> Self {
        Self::NextUpgrade
    }
}

impl From<(PublicKey, Option<TimeDiff>)> for PayloadType {
    fn from(_: (PublicKey, Option<TimeDiff>)) -> Self {
        Self::ConsensusStatus
    }
}

#[cfg(any(feature = "std", test))]
impl From<crate::ChainspecRawBytes> for PayloadType {
    fn from(_: crate::ChainspecRawBytes) -> Self {
        Self::ChainspecRawBytes
    }
}

impl From<Uptime> for PayloadType {
    fn from(_: Uptime) -> Self {
        Self::Uptime
    }
}

impl From<LastProgress> for PayloadType {
    fn from(_: LastProgress) -> Self {
        Self::LastProgress
    }
}

impl From<SpeculativeExecutionResult> for PayloadType {
    fn from(_: SpeculativeExecutionResult) -> Self {
        Self::SpeculativeExecutionResult
    }
}

impl From<GlobalStateQueryResult> for PayloadType {
    fn from(_: GlobalStateQueryResult) -> Self {
        Self::GlobalStateQueryResult
    }
}

impl From<StoredValues> for PayloadType {
    fn from(_: StoredValues) -> Self {
        Self::StoredValues
    }
}

impl From<GetTrieFullResult> for PayloadType {
    fn from(_: GetTrieFullResult) -> Self {
        Self::GetTrieFullResult
    }
}

#[cfg(any(feature = "std", test))]
impl From<super::NodeStatus> for PayloadType {
    fn from(_: super::NodeStatus) -> Self {
        Self::NodeStatus
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
const VEC_TRANSFERS_TAG: u8 = 11;
const VEC_U8_TAG: u8 = 12;
const FINALIZED_DEPLOY_APPROVALS_TAG: u8 = 13;
const FINALIZED_APPROVALS_TAG: u8 = 14;
const BLOCK_HASH_AND_HEIGHT_TAG: u8 = 15;
const BLOCK_HASH_TAG: u8 = 16;
const PEERS_MAP_TAG: u8 = 17;
const UPTIME_TAG: u8 = 18;
const LAST_PROGRESS_TAG: u8 = 19;
const REACTOR_STATE_TAG: u8 = 20;
const NETWORK_NAME_TAG: u8 = 21;
const CONSENSUS_VALIDATOR_CHANGES_TAG: u8 = 22;
const BLOCK_SYNCHRONIZER_STATUS_TAG: u8 = 23;
const AVAILABLE_BLOCK_RANGE_TAG: u8 = 24;
const NEXT_UPGRADE_TAG: u8 = 25;
const CONSENSUS_STATUS_TAG: u8 = 26;
const CHAINSPEC_RAW_BYTES_TAG: u8 = 27;
const HIGHEST_BLOCK_SEQUENCE_CHECK_RESULT_TAG: u8 = 28;
const SPECULATIVE_EXECUTION_RESULT_TAG: u8 = 29;
const GLOBAL_STATE_QUERY_RESULT_TAG: u8 = 30;
const STORED_VALUES_TAG: u8 = 31;
const GET_TRIE_FULL_RESULT_TAG: u8 = 32;
const NODE_STATUS_TAG: u8 = 33;

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
            PayloadType::VecTransfers => VEC_TRANSFERS_TAG,
            PayloadType::VecU8 => VEC_U8_TAG,
            PayloadType::FinalizedDeployApprovals => FINALIZED_DEPLOY_APPROVALS_TAG,
            PayloadType::FinalizedApprovals => FINALIZED_APPROVALS_TAG,
            PayloadType::BlockHashAndHeight => BLOCK_HASH_AND_HEIGHT_TAG,
            PayloadType::BlockHash => BLOCK_HASH_TAG,
            PayloadType::Peers => PEERS_MAP_TAG,
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
        let db_id = match tag {
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
            VEC_TRANSFERS_TAG => PayloadType::VecTransfers,
            VEC_U8_TAG => PayloadType::VecU8,
            FINALIZED_DEPLOY_APPROVALS_TAG => PayloadType::FinalizedDeployApprovals,
            FINALIZED_APPROVALS_TAG => PayloadType::FinalizedApprovals,
            BLOCK_HASH_AND_HEIGHT_TAG => PayloadType::BlockHashAndHeight,
            BLOCK_HASH_TAG => PayloadType::BlockHash,
            PEERS_MAP_TAG => PayloadType::Peers,
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

        let val = PayloadType::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
