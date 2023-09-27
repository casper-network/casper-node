use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use crate::{
    block::{BlockHash, EraEndV1},
    bytesrepr::{self, FromBytes, ToBytes},
    Digest, EraId, ProtocolVersion, PublicKey, Timestamp, U512,
};
#[cfg(feature = "std")]
use crate::{ActivationPoint, ProtocolConfig};

#[cfg(feature = "json-schema")]
static BLOCK_HEADER_V1: Lazy<BlockHeaderV1> = Lazy::new(|| {
    let parent_hash = BlockHash::new(Digest::from([7; Digest::LENGTH]));
    let state_root_hash = Digest::from([8; Digest::LENGTH]);
    let random_bit = true;
    let era_end = Some(EraEndV1::example().clone());
    let timestamp = *Timestamp::example();
    let era_id = EraId::from(1);
    let height: u64 = 10;
    let protocol_version = ProtocolVersion::V1_0_0;
    let accumulated_seed = Digest::hash_pair(Digest::from([9; Digest::LENGTH]), [random_bit as u8]);
    let body_hash = Digest::from([5; Digest::LENGTH]);
    BlockHeaderV1::new(
        parent_hash,
        state_root_hash,
        body_hash,
        random_bit,
        accumulated_seed,
        era_end,
        timestamp,
        era_id,
        height,
        protocol_version,
        #[cfg(any(feature = "once_cell", test))]
        OnceCell::new(),
    )
});

/// The header portion of a block.
#[derive(Clone, Debug, Eq)]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct BlockHeaderV1 {
    /// The parent block's hash.
    pub(super) parent_hash: BlockHash,
    /// The root hash of global state after the deploys in this block have been executed.
    pub(super) state_root_hash: Digest,
    /// The hash of the block's body.
    pub(super) body_hash: Digest,
    /// A random bit needed for initializing a future era.
    pub(super) random_bit: bool,
    /// A seed needed for initializing a future era.
    pub(super) accumulated_seed: Digest,
    /// The `EraEnd` of a block if it is a switch block.
    pub(super) era_end: Option<EraEndV1>,
    /// The timestamp from when the block was proposed.
    pub(super) timestamp: Timestamp,
    /// The era ID in which this block was created.
    pub(super) era_id: EraId,
    /// The height of this block, i.e. the number of ancestors.
    pub(super) height: u64,
    /// The protocol version of the network from when this block was created.
    pub(super) protocol_version: ProtocolVersion,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    pub(super) block_hash: OnceCell<BlockHash>,
}

impl BlockHeaderV1 {
    /// Returns the hash of this block header.
    pub fn block_hash(&self) -> BlockHash {
        #[cfg(any(feature = "once_cell", test))]
        return *self.block_hash.get_or_init(|| self.compute_block_hash());

        #[cfg(not(any(feature = "once_cell", test)))]
        self.compute_block_hash()
    }

    /// Returns the parent block's hash.
    pub fn parent_hash(&self) -> &BlockHash {
        &self.parent_hash
    }

    /// Returns the root hash of global state after the deploys in this block have been executed.
    pub fn state_root_hash(&self) -> &Digest {
        &self.state_root_hash
    }

    /// Returns the hash of the block's body.
    pub fn body_hash(&self) -> &Digest {
        &self.body_hash
    }

    /// Returns a random bit needed for initializing a future era.
    pub fn random_bit(&self) -> bool {
        self.random_bit
    }

    /// Returns a seed needed for initializing a future era.
    pub fn accumulated_seed(&self) -> &Digest {
        &self.accumulated_seed
    }

    /// Returns the `EraEnd` of a block if it is a switch block.
    pub fn era_end(&self) -> Option<&EraEndV1> {
        self.era_end.as_ref()
    }

    /// Returns the timestamp from when the block was proposed.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the era ID in which this block was created.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the era ID in which the next block would be created (i.e. this block's era ID, or
    /// its successor if this is a switch block).
    pub fn next_block_era_id(&self) -> EraId {
        if self.era_end.is_some() {
            self.era_id.successor()
        } else {
            self.era_id
        }
    }

    /// Returns the height of this block, i.e. the number of ancestors.
    pub fn height(&self) -> u64 {
        self.height
    }

    /// Returns the protocol version of the network from when this block was created.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns `true` if this block is the last one in the current era.
    pub fn is_switch_block(&self) -> bool {
        self.era_end.is_some()
    }

    /// Returns the validators for the upcoming era and their respective weights (if this is a
    /// switch block).
    pub fn next_era_validator_weights(&self) -> Option<&BTreeMap<PublicKey, U512>> {
        self.era_end
            .as_ref()
            .map(|era_end| era_end.next_era_validator_weights())
    }

    /// Returns `true` if this block is the Genesis block, i.e. has height 0 and era 0.
    pub fn is_genesis(&self) -> bool {
        self.era_id().is_genesis() && self.height() == 0
    }

    /// Returns `true` if this block belongs to the last block before the upgrade to the
    /// current protocol version.
    #[cfg(feature = "std")]
    pub fn is_last_block_before_activation(&self, protocol_config: &ProtocolConfig) -> bool {
        protocol_config.version > self.protocol_version
            && self.is_switch_block()
            && ActivationPoint::EraId(self.next_block_era_id()) == protocol_config.activation_point
    }

    pub(crate) fn compute_block_hash(&self) -> BlockHash {
        let serialized_header = self
            .to_bytes()
            .unwrap_or_else(|error| panic!("should serialize block header: {}", error));
        BlockHash::new(Digest::hash(serialized_header))
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        parent_hash: BlockHash,
        state_root_hash: Digest,
        body_hash: Digest,
        random_bit: bool,
        accumulated_seed: Digest,
        era_end: Option<EraEndV1>,
        timestamp: Timestamp,
        era_id: EraId,
        height: u64,
        protocol_version: ProtocolVersion,
        #[cfg(any(feature = "once_cell", test))] block_hash: OnceCell<BlockHash>,
    ) -> Self {
        BlockHeaderV1 {
            parent_hash,
            state_root_hash,
            body_hash,
            random_bit,
            accumulated_seed,
            era_end,
            timestamp,
            era_id,
            height,
            protocol_version,
            #[cfg(any(feature = "once_cell", test))]
            block_hash,
        }
    }

    // This method is not intended to be used by third party crates.
    //
    // Sets the block hash without recomputing it. Must only be called with the correct hash.
    #[doc(hidden)]
    #[cfg(any(feature = "once_cell", test))]
    pub fn set_block_hash(&self, block_hash: BlockHash) {
        self.block_hash.get_or_init(|| block_hash);
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &BLOCK_HEADER_V1
    }

    #[cfg(test)]
    pub(crate) fn set_body_hash(&mut self, new_body_hash: Digest) {
        self.body_hash = new_body_hash;
    }
}

impl PartialEq for BlockHeaderV1 {
    fn eq(&self, other: &BlockHeaderV1) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let BlockHeaderV1 {
            parent_hash,
            state_root_hash,
            body_hash,
            random_bit,
            accumulated_seed,
            era_end,
            timestamp,
            era_id,
            height,
            protocol_version,
            block_hash: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let BlockHeaderV1 {
            parent_hash,
            state_root_hash,
            body_hash,
            random_bit,
            accumulated_seed,
            era_end,
            timestamp,
            era_id,
            height,
            protocol_version,
        } = self;
        *parent_hash == other.parent_hash
            && *state_root_hash == other.state_root_hash
            && *body_hash == other.body_hash
            && *random_bit == other.random_bit
            && *accumulated_seed == other.accumulated_seed
            && *era_end == other.era_end
            && *timestamp == other.timestamp
            && *era_id == other.era_id
            && *height == other.height
            && *protocol_version == other.protocol_version
    }
}

impl Display for BlockHeaderV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block header #{}, {}, timestamp {}, {}, parent {}, post-state hash {}, body hash {}, \
            random bit {}, protocol version: {}",
            self.height,
            self.block_hash(),
            self.timestamp,
            self.era_id,
            self.parent_hash.inner(),
            self.state_root_hash,
            self.body_hash,
            self.random_bit,
            self.protocol_version,
        )?;
        if let Some(era_end) = &self.era_end {
            write!(formatter, ", era_end: {}", era_end)?;
        }
        Ok(())
    }
}

impl ToBytes for BlockHeaderV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.parent_hash.write_bytes(writer)?;
        self.state_root_hash.write_bytes(writer)?;
        self.body_hash.write_bytes(writer)?;
        self.random_bit.write_bytes(writer)?;
        self.accumulated_seed.write_bytes(writer)?;
        self.era_end.write_bytes(writer)?;
        self.timestamp.write_bytes(writer)?;
        self.era_id.write_bytes(writer)?;
        self.height.write_bytes(writer)?;
        self.protocol_version.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.parent_hash.serialized_length()
            + self.state_root_hash.serialized_length()
            + self.body_hash.serialized_length()
            + self.random_bit.serialized_length()
            + self.accumulated_seed.serialized_length()
            + self.era_end.serialized_length()
            + self.timestamp.serialized_length()
            + self.era_id.serialized_length()
            + self.height.serialized_length()
            + self.protocol_version.serialized_length()
    }
}

impl FromBytes for BlockHeaderV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (parent_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (state_root_hash, remainder) = Digest::from_bytes(remainder)?;
        let (body_hash, remainder) = Digest::from_bytes(remainder)?;
        let (random_bit, remainder) = bool::from_bytes(remainder)?;
        let (accumulated_seed, remainder) = Digest::from_bytes(remainder)?;
        let (era_end, remainder) = Option::from_bytes(remainder)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (era_id, remainder) = EraId::from_bytes(remainder)?;
        let (height, remainder) = u64::from_bytes(remainder)?;
        let (protocol_version, remainder) = ProtocolVersion::from_bytes(remainder)?;
        let block_header = BlockHeaderV1 {
            parent_hash,
            state_root_hash,
            body_hash,
            random_bit,
            accumulated_seed,
            era_end,
            timestamp,
            era_id,
            height,
            protocol_version,
            #[cfg(any(feature = "once_cell", test))]
            block_hash: OnceCell::new(),
        };
        Ok((block_header, remainder))
    }
}
