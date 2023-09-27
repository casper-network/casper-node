#[cfg(any(all(feature = "std", feature = "testing"), test))]
use alloc::collections::BTreeMap;
use alloc::{boxed::Box, vec::Vec};
use core::fmt::{self, Display, Formatter};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use core::iter;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::Rng;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::U512;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Block, BlockBodyV1, BlockHash, BlockHeaderV1, BlockValidationError, DeployHash, Digest,
    EraEndV1, EraId, ProtocolVersion, PublicKey, Timestamp,
};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::{testing::TestRng, EraReport};

/// A block after execution, with the resulting global state root hash. This is the core component
/// of the Casper linear blockchain. Version 1.
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[derive(Clone, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct BlockV1 {
    /// The block hash identifying this block.
    pub(super) hash: BlockHash,
    /// The header portion of the block.
    pub(super) header: BlockHeaderV1,
    /// The body portion of the block.
    pub(super) body: BlockBodyV1,
}

impl BlockV1 {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        parent_hash: BlockHash,
        parent_seed: Digest,
        state_root_hash: Digest,
        random_bit: bool,
        era_end: Option<EraEndV1>,
        timestamp: Timestamp,
        era_id: EraId,
        height: u64,
        protocol_version: ProtocolVersion,
        proposer: PublicKey,
        deploy_hashes: Vec<DeployHash>,
        transfer_hashes: Vec<DeployHash>,
    ) -> Self {
        let body = BlockBodyV1::new(proposer, deploy_hashes, transfer_hashes);
        let body_hash = body.hash();
        let accumulated_seed = Digest::hash_pair(parent_seed, [random_bit as u8]);
        let header = BlockHeaderV1::new(
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
        );
        Self::new_from_header_and_body(header, body)
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn new_from_header_and_body(header: BlockHeaderV1, body: BlockBodyV1) -> Self {
        let hash = header.block_hash();
        BlockV1 { hash, header, body }
    }

    /// Returns the `BlockHash` identifying this block.
    pub fn hash(&self) -> &BlockHash {
        &self.hash
    }

    /// Returns the block's header.
    pub fn header(&self) -> &BlockHeaderV1 {
        &self.header
    }

    /// Returns the block's header, consuming `self`.
    pub fn take_header(self) -> BlockHeaderV1 {
        self.header
    }

    /// Returns the block's body.
    pub fn body(&self) -> &BlockBodyV1 {
        &self.body
    }

    /// Returns the parent block's hash.
    pub fn parent_hash(&self) -> &BlockHash {
        self.header.parent_hash()
    }

    /// Returns the root hash of global state after the deploys in this block have been executed.
    pub fn state_root_hash(&self) -> &Digest {
        self.header.state_root_hash()
    }

    /// Returns the hash of the block's body.
    pub fn body_hash(&self) -> &Digest {
        self.header.body_hash()
    }

    /// Returns a random bit needed for initializing a future era.
    pub fn random_bit(&self) -> bool {
        self.header.random_bit()
    }

    /// Returns a seed needed for initializing a future era.
    pub fn accumulated_seed(&self) -> &Digest {
        self.header.accumulated_seed()
    }

    /// Returns the `EraEnd` of a block if it is a switch block.
    pub fn era_end(&self) -> Option<&EraEndV1> {
        self.header.era_end()
    }

    /// Returns the timestamp from when the block was proposed.
    pub fn timestamp(&self) -> Timestamp {
        self.header.timestamp()
    }

    /// Returns the era ID in which this block was created.
    pub fn era_id(&self) -> EraId {
        self.header.era_id()
    }

    /// Returns the height of this block, i.e. the number of ancestors.
    pub fn height(&self) -> u64 {
        self.header.height()
    }

    /// Returns the protocol version of the network from when this block was created.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.header.protocol_version()
    }

    /// Returns `true` if this block is the last one in the current era.
    pub fn is_switch_block(&self) -> bool {
        self.header.is_switch_block()
    }

    /// Returns `true` if this block is the Genesis block, i.e. has height 0 and era 0.
    pub fn is_genesis(&self) -> bool {
        self.header.is_genesis()
    }

    /// Returns the public key of the validator which proposed the block.
    pub fn proposer(&self) -> &PublicKey {
        self.body.proposer()
    }

    /// Returns the deploy hashes within the block.
    pub fn deploy_hashes(&self) -> &[DeployHash] {
        self.body.deploy_hashes()
    }

    /// Returns the transfer hashes within the block.
    pub fn transfer_hashes(&self) -> &[DeployHash] {
        self.body.transfer_hashes()
    }

    /// Returns the deploy and transfer hashes in the order in which they were executed.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes()
            .iter()
            .chain(self.transfer_hashes().iter())
    }

    /// Returns `Ok` if and only if the block's provided block hash and body hash are identical to
    /// those generated by hashing the appropriate input data.
    pub fn verify(&self) -> Result<(), BlockValidationError> {
        let actual_block_header_hash = self.header().block_hash();
        if *self.hash() != actual_block_header_hash {
            return Err(BlockValidationError::UnexpectedBlockHash {
                block: Box::new(Block::V1(self.clone())),
                actual_block_hash: actual_block_header_hash,
            });
        }

        let actual_block_body_hash = self.body.hash();
        if *self.header.body_hash() != actual_block_body_hash {
            return Err(BlockValidationError::UnexpectedBodyHash {
                block: Box::new(Block::V1(self.clone())),
                actual_block_body_hash,
            });
        }

        Ok(())
    }

    /// Returns a random block, but using the provided values.
    ///
    /// If `deploy_hashes_iter` is empty, a few random deploy hashes will be added to the
    /// `deploy_hashes` and `transfer_hashes` fields of the body.  Otherwise, the provided deploy
    /// hashes will populate the `deploy_hashes` field and `transfer_hashes` will be empty.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random_with_specifics<I: IntoIterator<Item = DeployHash>>(
        rng: &mut TestRng,
        era_id: EraId,
        height: u64,
        protocol_version: ProtocolVersion,
        is_switch: bool,
        deploy_hashes_iter: I,
    ) -> Self {
        let parent_hash = BlockHash::random(rng);
        let parent_seed = Digest::random(rng);
        let state_root_hash = Digest::random(rng);
        let random_bit = rng.gen();
        let era_end = is_switch.then(|| {
            let mut next_era_validator_weights = BTreeMap::new();
            for i in 1_u64..6 {
                let _ = next_era_validator_weights.insert(PublicKey::random(rng), U512::from(i));
            }
            EraEndV1::new(EraReport::random(rng), next_era_validator_weights)
        });
        let timestamp = Timestamp::now();
        let proposer = PublicKey::random(rng);
        let mut deploy_hashes: Vec<DeployHash> = deploy_hashes_iter.into_iter().collect();
        let mut transfer_hashes: Vec<DeployHash> = vec![];
        if deploy_hashes.is_empty() {
            let count = rng.gen_range(0..6);
            deploy_hashes = iter::repeat_with(|| DeployHash::random(rng))
                .take(count)
                .collect();
            let count = rng.gen_range(0..6);
            transfer_hashes = iter::repeat_with(|| DeployHash::random(rng))
                .take(count)
                .collect();
        }

        BlockV1::new(
            parent_hash,
            parent_seed,
            state_root_hash,
            random_bit,
            era_end,
            timestamp,
            era_id,
            height,
            protocol_version,
            proposer,
            deploy_hashes,
            transfer_hashes,
        )
    }
}

impl Display for BlockV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "executed block #{}, {}, timestamp {}, {}, parent {}, post-state hash {}, body hash \
            {}, random bit {}, protocol version: {}",
            self.height(),
            self.hash(),
            self.timestamp(),
            self.era_id(),
            self.parent_hash().inner(),
            self.state_root_hash(),
            self.body_hash(),
            self.random_bit(),
            self.protocol_version()
        )?;
        if let Some(era_end) = self.era_end() {
            write!(formatter, ", era_end: {}", era_end)?;
        }
        Ok(())
    }
}

impl ToBytes for BlockV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.hash.write_bytes(writer)?;
        self.header.write_bytes(writer)?;
        self.body.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.hash.serialized_length()
            + self.header.serialized_length()
            + self.body.serialized_length()
    }
}

impl FromBytes for BlockV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (header, remainder) = BlockHeaderV1::from_bytes(remainder)?;
        let (body, remainder) = BlockBodyV1::from_bytes(remainder)?;
        let block = BlockV1 { hash, header, body };
        Ok((block, remainder))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Block, TestBlockV1Builder};

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let block = TestBlockV1Builder::new().build(rng);
        bytesrepr::test_serialization_roundtrip(&block);
    }

    #[test]
    fn block_check_bad_body_hash_sad_path() {
        let rng = &mut TestRng::new();

        let mut block = TestBlockV1Builder::new().build(rng);
        let bogus_block_body_hash = Digest::hash([0xde, 0xad, 0xbe, 0xef]);
        block.header.set_body_hash(bogus_block_body_hash);
        block.hash = block.header.block_hash();

        let expected_error = BlockValidationError::UnexpectedBodyHash {
            block: Box::new(Block::V1(block.clone())),
            actual_block_body_hash: block.body.hash(),
        };
        assert_eq!(block.verify(), Err(expected_error));
    }

    #[test]
    fn block_check_bad_block_hash_sad_path() {
        let rng = &mut TestRng::new();

        let mut block = TestBlockV1Builder::new().build(rng);
        let bogus_block_hash = BlockHash::from(Digest::hash([0xde, 0xad, 0xbe, 0xef]));
        block.hash = bogus_block_hash;

        let expected_error = BlockValidationError::UnexpectedBlockHash {
            block: Box::new(Block::V1(block.clone())),
            actual_block_hash: block.header.block_hash(),
        };
        assert_eq!(block.verify(), Err(expected_error));
    }
}
