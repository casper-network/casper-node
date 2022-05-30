use casper_types::EraId;

use crate::{
    storage::StorageRequest,
    types::{BlockHash, DeployHash},
};

/// An identifier used when getting a block or block header from storage.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(super) enum BlockGetterId {
    BlockHash {
        block_hash: BlockHash,
        only_from_available_range: bool,
    },
    BlockHeight {
        block_height: u64,
        only_from_available_range: bool,
    },
    HighestBlock,
    SwitchBlock(EraId),
    DeployHash(DeployHash),
    None,
}

impl BlockGetterId {
    pub(super) fn with_block_hash(block_hash: BlockHash) -> Self {
        BlockGetterId::BlockHash {
            block_hash,
            only_from_available_range: false,
        }
    }

    pub(super) fn with_block_hash_from_avail_range(
        block_hash: BlockHash,
        only_from_available_range: bool,
    ) -> Self {
        BlockGetterId::BlockHash {
            block_hash,
            only_from_available_range,
        }
    }

    pub(super) fn with_block_height(block_height: u64) -> Self {
        BlockGetterId::BlockHeight {
            block_height,
            only_from_available_range: false,
        }
    }

    pub(super) fn with_block_height_from_avail_range(
        block_height: u64,
        only_from_available_range: bool,
    ) -> Self {
        BlockGetterId::BlockHeight {
            block_height,
            only_from_available_range,
        }
    }
}

impl From<&StorageRequest> for BlockGetterId {
    fn from(request: &StorageRequest) -> Self {
        match request {
            StorageRequest::GetBlock { block_hash, .. }
            | StorageRequest::GetBlockAndDeploys { block_hash, .. } => {
                BlockGetterId::with_block_hash(*block_hash)
            }
            StorageRequest::GetBlockHeader {
                block_hash,
                only_from_available_block_range,
                ..
            }
            | StorageRequest::GetBlockAndMetadataByHash {
                block_hash,
                only_from_available_block_range,
                ..
            } => BlockGetterId::with_block_hash_from_avail_range(
                *block_hash,
                *only_from_available_block_range,
            ),
            StorageRequest::GetBlockHeaderAndSufficientFinalitySignaturesByHeight {
                block_height,
                ..
            }
            | StorageRequest::GetBlockAndSufficientFinalitySignaturesByHeight {
                block_height,
                ..
            } => BlockGetterId::with_block_height(*block_height),
            StorageRequest::GetBlockHeaderByHeight {
                block_height,
                only_from_available_block_range,
                ..
            }
            | StorageRequest::GetBlockAndMetadataByHeight {
                block_height,
                only_from_available_block_range,
                ..
            } => BlockGetterId::with_block_height_from_avail_range(
                *block_height,
                *only_from_available_block_range,
            ),
            StorageRequest::GetHighestBlock { .. }
            | StorageRequest::GetHighestBlockHeader { .. }
            | StorageRequest::GetHighestBlockWithMetadata { .. } => BlockGetterId::HighestBlock,
            StorageRequest::GetSwitchBlockHeaderAtEraId { era_id, .. } => {
                BlockGetterId::SwitchBlock(*era_id)
            }
            StorageRequest::GetBlockHeaderForDeploy { deploy_hash, .. } => {
                BlockGetterId::DeployHash(*deploy_hash)
            }
            StorageRequest::PutBlock { .. }
            | StorageRequest::PutBlockHeader { .. }
            | StorageRequest::PutDeploy { .. }
            | StorageRequest::PutExecutionResults { .. }
            | StorageRequest::PutBlockSignatures { .. }
            | StorageRequest::PutBlockAndDeploys { .. }
            | StorageRequest::StoreFinalizedApprovals { .. }
            | StorageRequest::CheckBlockHeaderExistence { .. }
            | StorageRequest::GetBlockTransfers { .. }
            | StorageRequest::GetBlockSignatures { .. }
            | StorageRequest::GetFinalizedBlocks { .. }
            | StorageRequest::GetDeploys { .. }
            | StorageRequest::GetDeployAndMetadata { .. }
            | StorageRequest::UpdateLowestAvailableBlockHeight { .. }
            | StorageRequest::GetAvailableBlockRange { .. } => BlockGetterId::None,
        }
    }
}
