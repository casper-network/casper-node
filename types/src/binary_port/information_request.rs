use alloc::vec::Vec;
use core::convert::TryFrom;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    BlockIdentifier, TransactionHash,
};

use super::GetRequest;

/// Request for information from the node.
#[derive(Clone, Debug, PartialEq)]
pub enum InformationRequest {
    /// Returns the block header by an identifier, no identifier indicates the latest block.
    BlockHeader(Option<BlockIdentifier>),
    /// Returns the signed block by an identifier, no identifier indicates the latest block.
    SignedBlock(Option<BlockIdentifier>),
    /// Returns a transaction with approvals and execution info for a given hash.
    Transaction(TransactionHash),
    /// Returns connected peers.
    Peers,
    /// Returns node uptime.
    Uptime,
    /// Returns last progress of the sync process.
    LastProgress,
    /// Returns current state of the main reactor.
    ReactorState,
    /// Returns network name.
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

impl InformationRequest {
    /// Returns the tag of the request.
    pub fn tag(&self) -> InformationRequestTag {
        match self {
            InformationRequest::BlockHeader(_) => InformationRequestTag::BlockHeader,
            InformationRequest::SignedBlock(_) => InformationRequestTag::SignedBlock,
            InformationRequest::Transaction(_) => InformationRequestTag::Transaction,
            InformationRequest::Peers => InformationRequestTag::Peers,
            InformationRequest::Uptime => InformationRequestTag::Uptime,
            InformationRequest::LastProgress => InformationRequestTag::LastProgress,
            InformationRequest::ReactorState => InformationRequestTag::ReactorState,
            InformationRequest::NetworkName => InformationRequestTag::NetworkName,
            InformationRequest::ConsensusValidatorChanges => {
                InformationRequestTag::ConsensusValidatorChanges
            }
            InformationRequest::BlockSynchronizerStatus => {
                InformationRequestTag::BlockSynchronizerStatus
            }
            InformationRequest::AvailableBlockRange => InformationRequestTag::AvailableBlockRange,
            InformationRequest::NextUpgrade => InformationRequestTag::NextUpgrade,
            InformationRequest::ConsensusStatus => InformationRequestTag::ConsensusStatus,
            InformationRequest::ChainspecRawBytes => InformationRequestTag::ChainspecRawBytes,
            InformationRequest::NodeStatus => InformationRequestTag::NodeStatus,
        }
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match InformationRequestTag::random(rng) {
            InformationRequestTag::BlockHeader => {
                if rng.gen() {
                    InformationRequest::BlockHeader(None)
                } else {
                    InformationRequest::BlockHeader(Some(BlockIdentifier::random(rng)))
                }
            }
            InformationRequestTag::SignedBlock => {
                if rng.gen() {
                    InformationRequest::SignedBlock(None)
                } else {
                    InformationRequest::SignedBlock(Some(BlockIdentifier::random(rng)))
                }
            }
            InformationRequestTag::Transaction => {
                InformationRequest::Transaction(TransactionHash::random(rng))
            }
            InformationRequestTag::Peers => InformationRequest::Peers,
            InformationRequestTag::Uptime => InformationRequest::Uptime,
            InformationRequestTag::LastProgress => InformationRequest::LastProgress,
            InformationRequestTag::ReactorState => InformationRequest::ReactorState,
            InformationRequestTag::NetworkName => InformationRequest::NetworkName,
            InformationRequestTag::ConsensusValidatorChanges => {
                InformationRequest::ConsensusValidatorChanges
            }
            InformationRequestTag::BlockSynchronizerStatus => {
                InformationRequest::BlockSynchronizerStatus
            }
            InformationRequestTag::AvailableBlockRange => InformationRequest::AvailableBlockRange,
            InformationRequestTag::NextUpgrade => InformationRequest::NextUpgrade,
            InformationRequestTag::ConsensusStatus => InformationRequest::ConsensusStatus,
            InformationRequestTag::ChainspecRawBytes => InformationRequest::ChainspecRawBytes,
            InformationRequestTag::NodeStatus => InformationRequest::NodeStatus,
        }
    }
}

impl ToBytes for InformationRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            InformationRequest::BlockHeader(block_identifier) => {
                block_identifier.write_bytes(writer)
            }
            InformationRequest::SignedBlock(block_identifier) => {
                block_identifier.write_bytes(writer)
            }
            InformationRequest::Transaction(transaction_hash) => {
                transaction_hash.write_bytes(writer)
            }
            InformationRequest::Peers
            | InformationRequest::Uptime
            | InformationRequest::LastProgress
            | InformationRequest::ReactorState
            | InformationRequest::NetworkName
            | InformationRequest::ConsensusValidatorChanges
            | InformationRequest::BlockSynchronizerStatus
            | InformationRequest::AvailableBlockRange
            | InformationRequest::NextUpgrade
            | InformationRequest::ConsensusStatus
            | InformationRequest::ChainspecRawBytes
            | InformationRequest::NodeStatus => Ok(()),
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            InformationRequest::BlockHeader(block_identifier) => {
                block_identifier.serialized_length()
            }
            InformationRequest::SignedBlock(block_identifier) => {
                block_identifier.serialized_length()
            }
            InformationRequest::Transaction(transaction_hash) => {
                transaction_hash.serialized_length()
            }
            InformationRequest::Peers
            | InformationRequest::Uptime
            | InformationRequest::LastProgress
            | InformationRequest::ReactorState
            | InformationRequest::NetworkName
            | InformationRequest::ConsensusValidatorChanges
            | InformationRequest::BlockSynchronizerStatus
            | InformationRequest::AvailableBlockRange
            | InformationRequest::NextUpgrade
            | InformationRequest::ConsensusStatus
            | InformationRequest::ChainspecRawBytes
            | InformationRequest::NodeStatus => 0,
        }
    }
}

impl TryFrom<(InformationRequestTag, &[u8])> for InformationRequest {
    type Error = bytesrepr::Error;

    fn try_from((tag, key_bytes): (InformationRequestTag, &[u8])) -> Result<Self, Self::Error> {
        let (req, remainder) = match tag {
            InformationRequestTag::BlockHeader => {
                let (block_identifier, remainder) = FromBytes::from_bytes(key_bytes)?;
                (InformationRequest::BlockHeader(block_identifier), remainder)
            }
            InformationRequestTag::SignedBlock => {
                let (block_identifier, remainder) = FromBytes::from_bytes(key_bytes)?;
                (InformationRequest::SignedBlock(block_identifier), remainder)
            }
            InformationRequestTag::Transaction => {
                let (transaction_hash, remainder) = FromBytes::from_bytes(key_bytes)?;
                (InformationRequest::Transaction(transaction_hash), remainder)
            }
            InformationRequestTag::Peers => (InformationRequest::Peers, key_bytes),
            InformationRequestTag::Uptime => (InformationRequest::Uptime, key_bytes),
            InformationRequestTag::LastProgress => (InformationRequest::LastProgress, key_bytes),
            InformationRequestTag::ReactorState => (InformationRequest::ReactorState, key_bytes),
            InformationRequestTag::NetworkName => (InformationRequest::NetworkName, key_bytes),
            InformationRequestTag::ConsensusValidatorChanges => {
                (InformationRequest::ConsensusValidatorChanges, key_bytes)
            }
            InformationRequestTag::BlockSynchronizerStatus => {
                (InformationRequest::BlockSynchronizerStatus, key_bytes)
            }
            InformationRequestTag::AvailableBlockRange => {
                (InformationRequest::AvailableBlockRange, key_bytes)
            }
            InformationRequestTag::NextUpgrade => (InformationRequest::NextUpgrade, key_bytes),
            InformationRequestTag::ConsensusStatus => {
                (InformationRequest::ConsensusStatus, key_bytes)
            }
            InformationRequestTag::ChainspecRawBytes => {
                (InformationRequest::ChainspecRawBytes, key_bytes)
            }
            InformationRequestTag::NodeStatus => (InformationRequest::NodeStatus, key_bytes),
        };
        if !remainder.is_empty() {
            return Err(bytesrepr::Error::LeftOverBytes);
        }
        Ok(req)
    }
}

impl TryFrom<InformationRequest> for GetRequest {
    type Error = bytesrepr::Error;

    fn try_from(request: InformationRequest) -> Result<Self, Self::Error> {
        Ok(GetRequest::Information {
            info_type_tag: request.tag().into(),
            key: request.to_bytes()?,
        })
    }
}

/// Identifier of an information request.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u16)]
pub enum InformationRequestTag {
    /// Block header request.
    BlockHeader = 0,
    /// Signed block request.
    SignedBlock = 1,
    /// Transaction request.
    Transaction = 2,
    /// Peers request.
    Peers = 3,
    /// Uptime request.
    Uptime = 4,
    /// Last progress request.
    LastProgress = 5,
    /// Reactor state request.
    ReactorState = 6,
    /// Network name request.
    NetworkName = 7,
    /// Consensus validator changes request.
    ConsensusValidatorChanges = 8,
    /// Block synchronizer status request.
    BlockSynchronizerStatus = 9,
    /// Available block range request.
    AvailableBlockRange = 10,
    /// Next upgrade request.
    NextUpgrade = 11,
    /// Consensus status request.
    ConsensusStatus = 12,
    /// Chainspec raw bytes request.
    ChainspecRawBytes = 13,
    /// Node status request.
    NodeStatus = 14,
}

impl InformationRequestTag {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..15) {
            0 => InformationRequestTag::BlockHeader,
            1 => InformationRequestTag::SignedBlock,
            2 => InformationRequestTag::Transaction,
            3 => InformationRequestTag::Peers,
            4 => InformationRequestTag::Uptime,
            5 => InformationRequestTag::LastProgress,
            6 => InformationRequestTag::ReactorState,
            7 => InformationRequestTag::NetworkName,
            8 => InformationRequestTag::ConsensusValidatorChanges,
            9 => InformationRequestTag::BlockSynchronizerStatus,
            10 => InformationRequestTag::AvailableBlockRange,
            11 => InformationRequestTag::NextUpgrade,
            12 => InformationRequestTag::ConsensusStatus,
            13 => InformationRequestTag::ChainspecRawBytes,
            14 => InformationRequestTag::NodeStatus,
            _ => unreachable!(),
        }
    }
}

impl TryFrom<u16> for InformationRequestTag {
    type Error = UnknownInformationRequestTag;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(InformationRequestTag::BlockHeader),
            1 => Ok(InformationRequestTag::SignedBlock),
            2 => Ok(InformationRequestTag::Transaction),
            3 => Ok(InformationRequestTag::Peers),
            4 => Ok(InformationRequestTag::Uptime),
            5 => Ok(InformationRequestTag::LastProgress),
            6 => Ok(InformationRequestTag::ReactorState),
            7 => Ok(InformationRequestTag::NetworkName),
            8 => Ok(InformationRequestTag::ConsensusValidatorChanges),
            9 => Ok(InformationRequestTag::BlockSynchronizerStatus),
            10 => Ok(InformationRequestTag::AvailableBlockRange),
            11 => Ok(InformationRequestTag::NextUpgrade),
            12 => Ok(InformationRequestTag::ConsensusStatus),
            13 => Ok(InformationRequestTag::ChainspecRawBytes),
            14 => Ok(InformationRequestTag::NodeStatus),
            _ => Err(UnknownInformationRequestTag(value)),
        }
    }
}

impl From<InformationRequestTag> for u16 {
    fn from(value: InformationRequestTag) -> Self {
        value as u16
    }
}

/// Error returned when trying to convert a `u16` into a `DbId`.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownInformationRequestTag(u16);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn tag_roundtrip() {
        let rng = &mut TestRng::new();

        let val = InformationRequestTag::random(rng);
        let tag = u16::from(val);
        assert_eq!(InformationRequestTag::try_from(tag), Ok(val));
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = InformationRequest::random(rng);
        let bytes = val.to_bytes().expect("should serialize");
        assert_eq!(
            InformationRequest::try_from((val.tag(), &bytes[..])),
            Ok(val)
        );
    }
}
