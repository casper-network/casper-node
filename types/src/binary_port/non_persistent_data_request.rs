//! The "request for non persistent data" variant of the request to the binary port.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockIdentifier, TransactionHash,
};
use alloc::vec::Vec;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

const BLOCK_HEIGHT_2_HASH_TAG: u8 = 0;
const HIGHEST_COMPLETE_BLOCK_TAG: u8 = 1;
const COMPLETED_BLOCK_CONTAINS_TAG: u8 = 2;
const TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG: u8 = 3;
const PEERS_TAG: u8 = 4;
const UPTIME_TAG: u8 = 5;
const LAST_PROGRESS_TAG: u8 = 6;
const REACTOR_STATE_TAG: u8 = 7;
const NETWORK_NAME_TAG: u8 = 8;
const CONSENSUS_VALIDATOR_CHANGES_TAG: u8 = 9;
const BLOCK_SYNCHRONIZER_STATUS_TAG: u8 = 10;
const AVAILABLE_BLOCK_RANGE_TAG: u8 = 11;
const NEXT_UPGRADE_TAG: u8 = 12;
const CONSENSUS_STATUS_TAG: u8 = 13;
const CHAINSPEC_RAW_BYTES: u8 = 14;
const STATUS_TAG: u8 = 15;

/// Request for non persistent data
#[derive(Clone, Debug, PartialEq)]
pub enum NonPersistedDataRequest {
    /// Returns hash for a given height.
    BlockHeight2Hash {
        /// Block height.
        height: u64,
    },
    /// Returns height&hash for the currently highest block.
    HighestCompleteBlock,
    /// Returns true if `self.completed_blocks.highest_sequence()` contains the given hash
    CompletedBlocksContain {
        /// Block identifier.
        block_identifier: BlockIdentifier,
    },
    /// Returns block hash and height for a given transaction hash.
    TransactionHash2BlockHashAndHeight {
        /// Transaction hash.
        transaction_hash: TransactionHash,
    },
    /// Returns connected peers.
    Peers,
    /// Returns node uptime.
    Uptime,
    /// Returns last progress of the sync process.
    LastProgress,
    /// Returns current state of the main reactor.
    ReactorState,
    /// Returns current network name.
    NetworkName,
    /// Returns consensus validator changes.
    ConsensusValidatorChanges,
    /// Returns status of the BlockSynchronizer.
    BlockSynchronizerStatus,
    /// Returns the available block range.
    AvailableBlockRange,
    /// Returns info about next upgrade.
    NextUpgrade,
    /// Returns consensus status.
    ConsensusStatus,
    /// Returns chainspec raw bytes.
    ChainspecRawBytes,
    /// Returns the status information of the node.
    NodeStatus,
}

impl NonPersistedDataRequest {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..16) {
            0 => Self::BlockHeight2Hash { height: rng.gen() },
            1 => Self::HighestCompleteBlock,
            2 => Self::CompletedBlocksContain {
                block_identifier: BlockIdentifier::random(rng),
            },
            3 => Self::TransactionHash2BlockHashAndHeight {
                transaction_hash: TransactionHash::random(rng),
            },
            4 => Self::Peers,
            5 => Self::Uptime,
            6 => Self::LastProgress,
            7 => Self::ReactorState,
            8 => Self::NetworkName,
            9 => Self::ConsensusValidatorChanges,
            10 => Self::BlockSynchronizerStatus,
            11 => Self::AvailableBlockRange,
            12 => Self::NextUpgrade,
            13 => Self::ConsensusStatus,
            14 => Self::ChainspecRawBytes,
            15 => Self::NodeStatus,
            _ => panic!(),
        }
    }
}

impl ToBytes for NonPersistedDataRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            NonPersistedDataRequest::BlockHeight2Hash { height } => {
                BLOCK_HEIGHT_2_HASH_TAG.write_bytes(writer)?;
                height.write_bytes(writer)
            }
            NonPersistedDataRequest::HighestCompleteBlock => {
                HIGHEST_COMPLETE_BLOCK_TAG.write_bytes(writer)
            }
            NonPersistedDataRequest::CompletedBlocksContain {
                block_identifier: block_hash,
            } => {
                COMPLETED_BLOCK_CONTAINS_TAG.write_bytes(writer)?;
                block_hash.write_bytes(writer)
            }
            NonPersistedDataRequest::TransactionHash2BlockHashAndHeight { transaction_hash } => {
                TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG.write_bytes(writer)?;
                transaction_hash.write_bytes(writer)
            }
            NonPersistedDataRequest::Peers => PEERS_TAG.write_bytes(writer),
            NonPersistedDataRequest::Uptime => UPTIME_TAG.write_bytes(writer),
            NonPersistedDataRequest::LastProgress => LAST_PROGRESS_TAG.write_bytes(writer),
            NonPersistedDataRequest::ReactorState => REACTOR_STATE_TAG.write_bytes(writer),
            NonPersistedDataRequest::NetworkName => NETWORK_NAME_TAG.write_bytes(writer),
            NonPersistedDataRequest::ConsensusValidatorChanges => {
                CONSENSUS_VALIDATOR_CHANGES_TAG.write_bytes(writer)
            }
            NonPersistedDataRequest::BlockSynchronizerStatus => {
                BLOCK_SYNCHRONIZER_STATUS_TAG.write_bytes(writer)
            }
            NonPersistedDataRequest::AvailableBlockRange => {
                AVAILABLE_BLOCK_RANGE_TAG.write_bytes(writer)
            }
            NonPersistedDataRequest::NextUpgrade => NEXT_UPGRADE_TAG.write_bytes(writer),
            NonPersistedDataRequest::ConsensusStatus => CONSENSUS_STATUS_TAG.write_bytes(writer),
            NonPersistedDataRequest::ChainspecRawBytes => CHAINSPEC_RAW_BYTES.write_bytes(writer),
            NonPersistedDataRequest::NodeStatus => STATUS_TAG.write_bytes(writer),
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                NonPersistedDataRequest::BlockHeight2Hash { height } => height.serialized_length(),
                NonPersistedDataRequest::CompletedBlocksContain {
                    block_identifier: block_hash,
                } => block_hash.serialized_length(),
                NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                    transaction_hash,
                } => transaction_hash.serialized_length(),
                NonPersistedDataRequest::HighestCompleteBlock
                | NonPersistedDataRequest::Peers
                | NonPersistedDataRequest::Uptime
                | NonPersistedDataRequest::LastProgress
                | NonPersistedDataRequest::ReactorState
                | NonPersistedDataRequest::NetworkName
                | NonPersistedDataRequest::ConsensusValidatorChanges
                | NonPersistedDataRequest::BlockSynchronizerStatus
                | NonPersistedDataRequest::AvailableBlockRange
                | NonPersistedDataRequest::NextUpgrade
                | NonPersistedDataRequest::ConsensusStatus
                | NonPersistedDataRequest::ChainspecRawBytes
                | NonPersistedDataRequest::NodeStatus => 0,
            }
    }
}

impl FromBytes for NonPersistedDataRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = FromBytes::from_bytes(bytes)?;
        match tag {
            BLOCK_HEIGHT_2_HASH_TAG => {
                let (height, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    NonPersistedDataRequest::BlockHeight2Hash { height },
                    remainder,
                ))
            }
            HIGHEST_COMPLETE_BLOCK_TAG => {
                Ok((NonPersistedDataRequest::HighestCompleteBlock, remainder))
            }
            COMPLETED_BLOCK_CONTAINS_TAG => {
                let (block_hash, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    NonPersistedDataRequest::CompletedBlocksContain {
                        block_identifier: block_hash,
                    },
                    remainder,
                ))
            }
            TRANSACTION_HASH_2_BLOCK_HASH_AND_HEIGHT_TAG => {
                let (transaction_hash, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                        transaction_hash,
                    },
                    remainder,
                ))
            }
            PEERS_TAG => Ok((NonPersistedDataRequest::Peers, remainder)),
            UPTIME_TAG => Ok((NonPersistedDataRequest::Uptime, remainder)),
            LAST_PROGRESS_TAG => Ok((NonPersistedDataRequest::LastProgress, remainder)),
            REACTOR_STATE_TAG => Ok((NonPersistedDataRequest::ReactorState, remainder)),
            NETWORK_NAME_TAG => Ok((NonPersistedDataRequest::NetworkName, remainder)),
            CONSENSUS_VALIDATOR_CHANGES_TAG => Ok((
                NonPersistedDataRequest::ConsensusValidatorChanges,
                remainder,
            )),
            BLOCK_SYNCHRONIZER_STATUS_TAG => {
                Ok((NonPersistedDataRequest::BlockSynchronizerStatus, remainder))
            }
            AVAILABLE_BLOCK_RANGE_TAG => {
                Ok((NonPersistedDataRequest::AvailableBlockRange, remainder))
            }
            NEXT_UPGRADE_TAG => Ok((NonPersistedDataRequest::NextUpgrade, remainder)),
            CONSENSUS_STATUS_TAG => Ok((NonPersistedDataRequest::ConsensusStatus, remainder)),
            CHAINSPEC_RAW_BYTES => Ok((NonPersistedDataRequest::ChainspecRawBytes, remainder)),
            STATUS_TAG => Ok((NonPersistedDataRequest::NodeStatus, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = NonPersistedDataRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
