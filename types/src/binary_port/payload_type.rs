//! The payload type.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    AvailableBlockRange, BlockHash, BlockHashAndHeight, BlockSynchronizerStatus, ChainspecRawBytes,
    NextUpgrade, Peers, PublicKey, ReactorState, TimeDiff, Uptime,
};

use super::{
    db_id::DbId,
    type_wrappers::{
        ConsensusValidatorChanges, HighestBlockSequenceCheckResult, LastProgress, NetworkName,
    },
};

#[repr(u8)]
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

impl From<NextUpgrade> for PayloadType {
    fn from(_: NextUpgrade) -> Self {
        Self::NextUpgrade
    }
}

impl From<(PublicKey, Option<TimeDiff>)> for PayloadType {
    fn from(_: (PublicKey, Option<TimeDiff>)) -> Self {
        Self::ConsensusStatus
    }
}

impl From<ChainspecRawBytes> for PayloadType {
    fn from(_: ChainspecRawBytes) -> Self {
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
            AVAILABLE_BLOCK_RANGE_TAG => PayloadType::AvailableBlockRange,
            NEXT_UPGRADE_TAG => PayloadType::NextUpgrade,
            CONSENSUS_STATUS_TAG => PayloadType::ConsensusStatus,
            CHAINSPEC_RAW_BYTES_TAG => PayloadType::ChainspecRawBytes,
            UPTIME_TAG => PayloadType::Uptime,
            HIGHEST_BLOCK_SEQUENCE_CHECK_RESULT_TAG => PayloadType::HighestBlockSequenceCheckResult,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((db_id, remainder))
    }
}
