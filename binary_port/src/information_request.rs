use core::convert::TryFrom;

#[cfg(test)]
use rand::Rng;

use crate::{get_request::GetRequest, EraIdentifier};
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contracts::{ContractHash, ContractPackageHash},
    BlockIdentifier, EntityAddr, GlobalStateIdentifier, PackageAddr, PublicKey, TransactionHash,
};

/// Request for information from the node.
#[derive(Clone, Debug, PartialEq)]
pub enum InformationRequest {
    /// Returns the block header by an identifier, no identifier indicates the latest block.
    BlockHeader(Option<BlockIdentifier>),
    /// Returns the signed block by an identifier, no identifier indicates the latest block.
    SignedBlock(Option<BlockIdentifier>),
    /// Returns a transaction with approvals and execution info for a given hash.
    Transaction {
        /// Hash of the transaction to retrieve.
        hash: TransactionHash,
        /// Whether to return the deploy with the finalized approvals substituted.
        with_finalized_approvals: bool,
    },
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
    /// Returns the latest switch block header.
    LatestSwitchBlockHeader,
    /// Returns the reward for a validator or a delegator in a specific era.
    Reward {
        /// Identifier of the era to get the reward for. Must point to either a switch block or
        /// a valid `EraId`. If `None`, the reward for the latest switch block is returned.
        era_identifier: Option<EraIdentifier>,
        /// Public key of the validator to get the reward for.
        validator: Box<PublicKey>,
        /// Public key of the delegator to get the reward for.
        /// If `None`, the reward for the validator is returned.
        delegator: Option<Box<PublicKey>>,
    },
    /// Returns the current Casper protocol version.
    ProtocolVersion,
    /// Returns the contract package by an identifier.
    Package {
        /// Global state identifier, `None` means "latest block state".
        state_identifier: Option<GlobalStateIdentifier>,
        /// Identifier of the contract package to retrieve.
        identifier: PackageIdentifier,
    },
    /// Returns the entity by an identifier.
    Entity {
        /// Global state identifier, `None` means "latest block state".
        state_identifier: Option<GlobalStateIdentifier>,
        /// Identifier of the entity to retrieve.
        identifier: EntityIdentifier,
        /// Whether to return the bytecode with the entity.
        include_bytecode: bool,
    },
}

impl InformationRequest {
    /// Returns the tag of the request.
    pub fn tag(&self) -> InformationRequestTag {
        match self {
            InformationRequest::BlockHeader(_) => InformationRequestTag::BlockHeader,
            InformationRequest::SignedBlock(_) => InformationRequestTag::SignedBlock,
            InformationRequest::Transaction { .. } => InformationRequestTag::Transaction,
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
            InformationRequest::LatestSwitchBlockHeader => {
                InformationRequestTag::LatestSwitchBlockHeader
            }
            InformationRequest::Reward { .. } => InformationRequestTag::Reward,
            InformationRequest::ProtocolVersion => InformationRequestTag::ProtocolVersion,
            InformationRequest::Package { .. } => InformationRequestTag::Package,
            InformationRequest::Entity { .. } => InformationRequestTag::Entity,
        }
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match InformationRequestTag::random(rng) {
            InformationRequestTag::BlockHeader => InformationRequest::BlockHeader(
                rng.gen::<bool>().then(|| BlockIdentifier::random(rng)),
            ),
            InformationRequestTag::SignedBlock => InformationRequest::SignedBlock(
                rng.gen::<bool>().then(|| BlockIdentifier::random(rng)),
            ),
            InformationRequestTag::Transaction => InformationRequest::Transaction {
                hash: TransactionHash::random(rng),
                with_finalized_approvals: rng.gen(),
            },
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
            InformationRequestTag::LatestSwitchBlockHeader => {
                InformationRequest::LatestSwitchBlockHeader
            }
            InformationRequestTag::Reward => InformationRequest::Reward {
                era_identifier: rng.gen::<bool>().then(|| EraIdentifier::random(rng)),
                validator: PublicKey::random(rng).into(),
                delegator: rng.gen::<bool>().then(|| PublicKey::random(rng).into()),
            },
            InformationRequestTag::ProtocolVersion => InformationRequest::ProtocolVersion,
            InformationRequestTag::Package => InformationRequest::Package {
                state_identifier: rng
                    .gen::<bool>()
                    .then(|| GlobalStateIdentifier::random(rng)),
                identifier: PackageIdentifier::random(rng),
            },
            InformationRequestTag::Entity => InformationRequest::Entity {
                state_identifier: rng
                    .gen::<bool>()
                    .then(|| GlobalStateIdentifier::random(rng)),
                identifier: EntityIdentifier::random(rng),
                include_bytecode: rng.gen(),
            },
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
            InformationRequest::Transaction {
                hash,
                with_finalized_approvals,
            } => {
                hash.write_bytes(writer)?;
                with_finalized_approvals.write_bytes(writer)
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
            | InformationRequest::NodeStatus
            | InformationRequest::LatestSwitchBlockHeader
            | InformationRequest::ProtocolVersion => Ok(()),
            InformationRequest::Reward {
                era_identifier,
                validator,
                delegator,
            } => {
                era_identifier.write_bytes(writer)?;
                validator.write_bytes(writer)?;
                delegator.as_deref().write_bytes(writer)?;
                Ok(())
            }
            InformationRequest::Package {
                state_identifier,
                identifier,
            } => {
                state_identifier.write_bytes(writer)?;
                identifier.write_bytes(writer)
            }
            InformationRequest::Entity {
                state_identifier,
                identifier,
                include_bytecode,
            } => {
                state_identifier.write_bytes(writer)?;
                identifier.write_bytes(writer)?;
                include_bytecode.write_bytes(writer)
            }
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
            InformationRequest::Transaction {
                hash,
                with_finalized_approvals,
            } => hash.serialized_length() + with_finalized_approvals.serialized_length(),
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
            | InformationRequest::NodeStatus
            | InformationRequest::LatestSwitchBlockHeader
            | InformationRequest::ProtocolVersion => 0,
            InformationRequest::Reward {
                era_identifier,
                validator,
                delegator,
            } => {
                era_identifier.serialized_length()
                    + validator.serialized_length()
                    + delegator.as_deref().serialized_length()
            }
            InformationRequest::Package {
                state_identifier,
                identifier,
            } => state_identifier.serialized_length() + identifier.serialized_length(),
            InformationRequest::Entity {
                state_identifier,
                identifier,
                include_bytecode,
            } => {
                state_identifier.serialized_length()
                    + identifier.serialized_length()
                    + include_bytecode.serialized_length()
            }
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
                let (hash, remainder) = FromBytes::from_bytes(key_bytes)?;
                let (with_finalized_approvals, remainder) = FromBytes::from_bytes(remainder)?;
                (
                    InformationRequest::Transaction {
                        hash,
                        with_finalized_approvals,
                    },
                    remainder,
                )
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
            InformationRequestTag::LatestSwitchBlockHeader => {
                (InformationRequest::LatestSwitchBlockHeader, key_bytes)
            }
            InformationRequestTag::Reward => {
                let (era_identifier, remainder) = <Option<EraIdentifier>>::from_bytes(key_bytes)?;
                let (validator, remainder) = PublicKey::from_bytes(remainder)?;
                let (delegator, remainder) = <Option<PublicKey>>::from_bytes(remainder)?;
                (
                    InformationRequest::Reward {
                        era_identifier,
                        validator: Box::new(validator),
                        delegator: delegator.map(Box::new),
                    },
                    remainder,
                )
            }
            InformationRequestTag::ProtocolVersion => {
                (InformationRequest::ProtocolVersion, key_bytes)
            }
            InformationRequestTag::Package => {
                let (state_identifier, remainder) = FromBytes::from_bytes(key_bytes)?;
                let (identifier, remainder) = FromBytes::from_bytes(remainder)?;
                (
                    InformationRequest::Package {
                        state_identifier,
                        identifier,
                    },
                    remainder,
                )
            }
            InformationRequestTag::Entity => {
                let (state_identifier, remainder) = FromBytes::from_bytes(key_bytes)?;
                let (identifier, remainder) = FromBytes::from_bytes(remainder)?;
                let (include_bytecode, remainder) = FromBytes::from_bytes(remainder)?;
                (
                    InformationRequest::Entity {
                        state_identifier,
                        identifier,
                        include_bytecode,
                    },
                    remainder,
                )
            }
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
    /// Latest switch block header request.
    LatestSwitchBlockHeader = 15,
    /// Reward for a validator or a delegator in a specific era.
    Reward = 16,
    /// Protocol version request.
    ProtocolVersion = 17,
    /// Contract package request.
    Package = 18,
    /// Addressable entity request.
    Entity = 19,
}

impl InformationRequestTag {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..20) {
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
            15 => InformationRequestTag::LatestSwitchBlockHeader,
            16 => InformationRequestTag::Reward,
            17 => InformationRequestTag::ProtocolVersion,
            18 => InformationRequestTag::Package,
            19 => InformationRequestTag::Entity,
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
            15 => Ok(InformationRequestTag::LatestSwitchBlockHeader),
            16 => Ok(InformationRequestTag::Reward),
            17 => Ok(InformationRequestTag::ProtocolVersion),
            18 => Ok(InformationRequestTag::Package),
            19 => Ok(InformationRequestTag::Entity),
            _ => Err(UnknownInformationRequestTag(value)),
        }
    }
}

impl From<InformationRequestTag> for u16 {
    fn from(value: InformationRequestTag) -> Self {
        value as u16
    }
}

/// Error returned when trying to convert a `u16` into a `RecordId`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownInformationRequestTag(u16);

#[derive(Debug, Clone, PartialEq)]
pub enum EntityIdentifier {
    ContractHash(ContractHash),
    AccountHash(AccountHash),
    PublicKey(PublicKey),
    EntityAddr(EntityAddr),
}

impl EntityIdentifier {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..4) {
            0 => EntityIdentifier::ContractHash(ContractHash::new(rng.gen())),
            1 => EntityIdentifier::PublicKey(PublicKey::random(rng)),
            2 => EntityIdentifier::AccountHash(AccountHash::new(rng.gen())),
            3 => EntityIdentifier::EntityAddr(rng.gen()),
            _ => unreachable!(),
        }
    }
}

impl FromBytes for EntityIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let (identifier, remainder) = match tag {
            0 => {
                let (hash, remainder) = FromBytes::from_bytes(remainder)?;
                (EntityIdentifier::ContractHash(hash), remainder)
            }
            1 => {
                let (key, remainder) = FromBytes::from_bytes(remainder)?;
                (EntityIdentifier::PublicKey(key), remainder)
            }
            2 => {
                let (hash, remainder) = FromBytes::from_bytes(remainder)?;
                (EntityIdentifier::AccountHash(hash), remainder)
            }
            3 => {
                let (entity, remainder) = FromBytes::from_bytes(remainder)?;
                (EntityIdentifier::EntityAddr(entity), remainder)
            }
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((identifier, remainder))
    }
}

impl ToBytes for EntityIdentifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let tag: u8 = match self {
            EntityIdentifier::ContractHash(_) => 0,
            EntityIdentifier::PublicKey(_) => 1,
            EntityIdentifier::AccountHash(_) => 2,
            EntityIdentifier::EntityAddr(_) => 3,
        };
        tag.write_bytes(writer)?;
        match self {
            EntityIdentifier::ContractHash(hash) => hash.write_bytes(writer),
            EntityIdentifier::PublicKey(key) => key.write_bytes(writer),
            EntityIdentifier::AccountHash(hash) => hash.write_bytes(writer),
            EntityIdentifier::EntityAddr(entity) => entity.write_bytes(writer),
        }
    }

    fn serialized_length(&self) -> usize {
        let identifier_length = match self {
            EntityIdentifier::ContractHash(hash) => hash.serialized_length(),
            EntityIdentifier::PublicKey(key) => key.serialized_length(),
            EntityIdentifier::AccountHash(hash) => hash.serialized_length(),
            EntityIdentifier::EntityAddr(entity) => entity.serialized_length(),
        };
        U8_SERIALIZED_LENGTH + identifier_length
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PackageIdentifier {
    ContractPackageHash(ContractPackageHash),
    PackageAddr(PackageAddr),
}

impl PackageIdentifier {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..2) {
            0 => PackageIdentifier::ContractPackageHash(ContractPackageHash::new(rng.gen())),
            1 => PackageIdentifier::PackageAddr(rng.gen()),
            _ => unreachable!(),
        }
    }
}

impl FromBytes for PackageIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let (identifier, remainder) = match tag {
            0 => {
                let (hash, remainder) = FromBytes::from_bytes(remainder)?;
                (PackageIdentifier::ContractPackageHash(hash), remainder)
            }
            1 => {
                let (addr, remainder) = FromBytes::from_bytes(remainder)?;
                (PackageIdentifier::PackageAddr(addr), remainder)
            }
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((identifier, remainder))
    }
}

impl ToBytes for PackageIdentifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let tag: u8 = match self {
            PackageIdentifier::ContractPackageHash(_) => 0,
            PackageIdentifier::PackageAddr(_) => 1,
        };
        tag.write_bytes(writer)?;
        match self {
            PackageIdentifier::ContractPackageHash(hash) => hash.write_bytes(writer),
            PackageIdentifier::PackageAddr(addr) => addr.write_bytes(writer),
        }
    }

    fn serialized_length(&self) -> usize {
        let identifier_length = match self {
            PackageIdentifier::ContractPackageHash(hash) => hash.serialized_length(),
            PackageIdentifier::PackageAddr(addr) => addr.serialized_length(),
        };
        U8_SERIALIZED_LENGTH + identifier_length
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

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

    #[test]
    fn entity_identifier_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = EntityIdentifier::random(rng);
        let bytes = val.to_bytes().expect("should serialize");
        assert_eq!(bytesrepr::deserialize_from_slice(bytes), Ok(val));
    }

    #[test]
    fn package_identifier_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = PackageIdentifier::random(rng);
        let bytes = val.to_bytes().expect("should serialize");
        assert_eq!(bytesrepr::deserialize_from_slice(bytes), Ok(val));
    }
}
