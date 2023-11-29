use alloc::{boxed::Box, vec::Vec};

use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;

use super::{Block, BlockBodyV2, BlockConversionError, RewardedSignatures};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
#[cfg(feature = "json-schema")]
use crate::TransactionV1Hash;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    BlockHash, BlockHeaderV2, BlockValidationError, Digest, EraEndV2, EraId, ProtocolVersion,
    PublicKey, Timestamp, TransactionHash,
};

#[cfg(feature = "json-schema")]
static BLOCK_V2: Lazy<BlockV2> = Lazy::new(|| {
    let parent_hash = BlockHash::new(Digest::from([7; Digest::LENGTH]));
    let parent_seed = Digest::from([9; Digest::LENGTH]);
    let state_root_hash = Digest::from([8; Digest::LENGTH]);
    let random_bit = true;
    let era_end = Some(EraEndV2::example().clone());
    let timestamp = *Timestamp::example();
    let era_id = EraId::from(1);
    let height = 10;
    let protocol_version = ProtocolVersion::V1_0_0;
    let secret_key = crate::SecretKey::example();
    let proposer = PublicKey::from(secret_key);
    let transfer_hashes = vec![TransactionHash::V1(TransactionV1Hash::new(Digest::from(
        [20; Digest::LENGTH],
    )))];
    let non_transfer_native_hashes = vec![TransactionHash::V1(TransactionV1Hash::new(
        Digest::from([21; Digest::LENGTH]),
    ))];
    let installer_upgrader_hashes = vec![TransactionHash::V1(TransactionV1Hash::new(
        Digest::from([22; Digest::LENGTH]),
    ))];
    let other_hashes = vec![TransactionHash::V1(TransactionV1Hash::new(Digest::from(
        [23; Digest::LENGTH],
    )))];
    let rewarded_signatures = RewardedSignatures::default();
    BlockV2::new(
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
        transfer_hashes,
        non_transfer_native_hashes,
        installer_upgrader_hashes,
        other_hashes,
        rewarded_signatures,
    )
});

/// A block after execution, with the resulting global state root hash. This is the core component
/// of the Casper linear blockchain. Version 2.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct BlockV2 {
    /// The block hash identifying this block.
    pub(super) hash: BlockHash,
    /// The header portion of the block.
    pub(super) header: BlockHeaderV2,
    /// The body portion of the block.
    pub(super) body: BlockBodyV2,
}

impl BlockV2 {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        parent_hash: BlockHash,
        parent_seed: Digest,
        state_root_hash: Digest,
        random_bit: bool,
        era_end: Option<EraEndV2>,
        timestamp: Timestamp,
        era_id: EraId,
        height: u64,
        protocol_version: ProtocolVersion,
        proposer: PublicKey,
        transfer: Vec<TransactionHash>,
        staking: Vec<TransactionHash>,
        install_upgrade: Vec<TransactionHash>,
        standard: Vec<TransactionHash>,
        rewarded_signatures: RewardedSignatures,
    ) -> Self {
        let body = BlockBodyV2::new(
            proposer,
            transfer,
            staking,
            install_upgrade,
            standard,
            rewarded_signatures,
        );
        let body_hash = body.hash();
        let accumulated_seed = Digest::hash_pair(parent_seed, [random_bit as u8]);
        let header = BlockHeaderV2::new(
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
    pub fn new_from_header_and_body(header: BlockHeaderV2, body: BlockBodyV2) -> Self {
        let hash = header.block_hash();
        BlockV2 { hash, header, body }
    }

    /// Returns the `BlockHash` identifying this block.
    pub fn hash(&self) -> &BlockHash {
        &self.hash
    }

    /// Returns the block's header.
    pub fn header(&self) -> &BlockHeaderV2 {
        &self.header
    }

    /// Returns the block's header, consuming `self`.
    pub fn take_header(self) -> BlockHeaderV2 {
        self.header
    }

    /// Returns the block's body.
    pub fn body(&self) -> &BlockBodyV2 {
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
    pub fn era_end(&self) -> Option<&EraEndV2> {
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

    /// List of identifiers for finality signatures for a particular past block.
    pub fn rewarded_signatures(&self) -> &RewardedSignatures {
        self.body.rewarded_signatures()
    }

    /// Returns the hashes of the transfer transactions within the block.
    pub fn transfer(&self) -> impl Iterator<Item = &TransactionHash> {
        self.body.transfer()
    }

    /// Returns the hashes of the non-transfer, native transactions within the block.
    pub fn staking(&self) -> impl Iterator<Item = &TransactionHash> {
        self.body.staking()
    }

    /// Returns the hashes of the installer/upgrader transactions within the block.
    pub fn install_upgrade(&self) -> impl Iterator<Item = &TransactionHash> {
        self.body.install_upgrade()
    }

    /// Returns the hashes of all other transactions within the block.
    pub fn standard(&self) -> impl Iterator<Item = &TransactionHash> {
        self.body.standard()
    }

    /// Returns all of the transaction hashes in the order in which they were executed.
    pub fn all_transactions(&self) -> impl Iterator<Item = &TransactionHash> {
        self.body.all_transactions()
    }

    /// Returns `Ok` if and only if the block's provided block hash and body hash are identical to
    /// those generated by hashing the appropriate input data.
    pub fn verify(&self) -> Result<(), BlockValidationError> {
        let actual_block_header_hash = self.header().block_hash();
        if *self.hash() != actual_block_header_hash {
            return Err(BlockValidationError::UnexpectedBlockHash {
                block: Box::new(Block::V2(self.clone())),
                actual_block_hash: actual_block_header_hash,
            });
        }

        let actual_block_body_hash = self.body.hash();
        if *self.header.body_hash() != actual_block_body_hash {
            return Err(BlockValidationError::UnexpectedBodyHash {
                block: Box::new(Block::V2(self.clone())),
                actual_block_body_hash,
            });
        }

        Ok(())
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &BLOCK_V2
    }

    /// Makes the block invalid, for testing purpose.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn make_invalid(self, rng: &mut TestRng) -> Self {
        let block = BlockV2 {
            hash: BlockHash::random(rng),
            ..self
        };

        assert!(block.verify().is_err());
        block
    }
}

impl Display for BlockV2 {
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

impl ToBytes for BlockV2 {
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

impl FromBytes for BlockV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (header, remainder) = BlockHeaderV2::from_bytes(remainder)?;
        let (body, remainder) = BlockBodyV2::from_bytes(remainder)?;
        let block = BlockV2 { hash, header, body };
        Ok((block, remainder))
    }
}

impl TryFrom<Block> for BlockV2 {
    type Error = BlockConversionError;

    fn try_from(value: Block) -> Result<BlockV2, BlockConversionError> {
        match value {
            Block::V2(v2) => Ok(v2),
            _ => Err(BlockConversionError::DifferentVersion {
                expected_version: 2,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::TestBlockBuilder;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let block = TestBlockBuilder::new().build(rng);
        bytesrepr::test_serialization_roundtrip(&block);
    }

    #[test]
    fn block_check_bad_body_hash_sad_path() {
        let rng = &mut TestRng::new();

        let mut block = TestBlockBuilder::new().build(rng);
        let bogus_block_body_hash = Digest::hash([0xde, 0xad, 0xbe, 0xef]);
        block.header.set_body_hash(bogus_block_body_hash);
        block.hash = block.header.block_hash();

        let expected_error = BlockValidationError::UnexpectedBodyHash {
            block: Box::new(Block::V2(block.clone())),
            actual_block_body_hash: block.body.hash(),
        };
        assert_eq!(block.verify(), Err(expected_error));
    }

    #[test]
    fn block_check_bad_block_hash_sad_path() {
        let rng = &mut TestRng::new();

        let mut block = TestBlockBuilder::new().build(rng);
        let bogus_block_hash = BlockHash::from(Digest::hash([0xde, 0xad, 0xbe, 0xef]));
        block.hash = bogus_block_hash;

        let expected_error = BlockValidationError::UnexpectedBlockHash {
            block: Box::new(Block::V2(block.clone())),
            actual_block_hash: block.header.block_hash(),
        };
        assert_eq!(block.verify(), Err(expected_error));
    }
}
