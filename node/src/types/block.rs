#[cfg(test)]
use std::iter;
use std::{
    array::TryFromSliceError,
    collections::BTreeMap,
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
};

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};

use datasize::DataSize;
use hex::FromHexError;
use hex_fmt::{HexFmt, HexList};
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(test)]
use casper_types::auction::BLOCK_REWARD;

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use super::{Item, Tag, Timestamp};
use crate::{
    components::consensus::{self, EraId},
    crypto::{
        asymmetric_key::{PublicKey, Signature},
        hash::{self, Digest},
    },
    types::DeployHash,
    utils::DisplayIter,
};
#[cfg(test)]
use crate::{
    crypto::asymmetric_key::{self, SecretKey},
    testing::TestRng,
};

/// Error returned from constructing or validating a `Block`.
#[derive(Debug, Error)]
pub enum Error {
    /// Error while encoding to JSON.
    #[error("encoding to JSON: {0}")]
    EncodeToJson(#[from] serde_json::Error),

    /// Error while decoding from JSON.
    #[error("decoding from JSON: {0}")]
    DecodeFromJson(Box<dyn StdError>),
}

impl From<FromHexError> for Error {
    fn from(error: FromHexError) -> Self {
        Error::DecodeFromJson(Box::new(error))
    }
}

impl From<TryFromSliceError> for Error {
    fn from(error: TryFromSliceError) -> Self {
        Error::DecodeFromJson(Box::new(error))
    }
}

pub trait BlockLike: Eq + Hash {
    fn deploys(&self) -> &Vec<DeployHash>;
}

/// A cryptographic hash identifying a `ProtoBlock`.
#[derive(
    Copy,
    Clone,
    DataSize,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Debug,
    Default,
)]
pub struct ProtoBlockHash(Digest);

impl ProtoBlockHash {
    /// Constructs a new `ProtoBlockHash`.
    pub fn new(hash: Digest) -> Self {
        ProtoBlockHash(hash)
    }

    /// Returns the wrapped inner hash.
    pub fn inner(&self) -> &Digest {
        &self.0
    }
}

impl Display for ProtoBlockHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "proto-block-hash({})", self.0)
    }
}

/// The piece of information that will become the content of a future block (isn't finalized or
/// executed yet)
///
/// From the view of the consensus protocol this is the "consensus value": The protocol deals with
/// finalizing an order of `ProtoBlock`s. Only after consensus has been reached, the block's
/// deploys actually get executed, and the executed block gets signed.
///
/// The word "proto" does _not_ refer to "protocol" or "protobuf"! It is just a prefix to highlight
/// that this comes before a block in the linear, executed, finalized blockchain is produced.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProtoBlock {
    hash: ProtoBlockHash,
    deploys: Vec<DeployHash>,
    random_bit: bool,
}

impl ProtoBlock {
    pub(crate) fn new(deploys: Vec<DeployHash>, random_bit: bool) -> Self {
        let hash = ProtoBlockHash::new(hash::hash(
            &bincode::serialize(&(&deploys, random_bit)).expect("serialize ProtoBlock"),
        ));

        ProtoBlock {
            hash,
            deploys,
            random_bit,
        }
    }

    pub(crate) fn hash(&self) -> &ProtoBlockHash {
        &self.hash
    }

    /// The list of deploy hashes included in the block.
    pub(crate) fn deploys(&self) -> &Vec<DeployHash> {
        &self.deploys
    }

    /// A random bit needed for initializing a future era.
    pub(crate) fn random_bit(&self) -> bool {
        self.random_bit
    }

    pub(crate) fn destructure(self) -> (ProtoBlockHash, Vec<DeployHash>, bool) {
        (self.hash, self.deploys, self.random_bit)
    }
}

impl Display for ProtoBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "proto block {}, deploys [{}], random bit {}",
            self.hash.inner(),
            DisplayIter::new(self.deploys.iter()),
            self.random_bit(),
        )
    }
}

impl BlockLike for ProtoBlock {
    fn deploys(&self) -> &Vec<DeployHash> {
        self.deploys()
    }
}

/// Equivocation and reward information to be included in the terminal finalized block.
pub type EraEnd = consensus::EraEnd<PublicKey>;

impl Display for EraEnd {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let slashings = DisplayIter::new(&self.equivocators);
        let rewards = DisplayIter::new(
            self.rewards
                .iter()
                .map(|(public_key, amount)| format!("{}: {}", public_key, amount)),
        );
        write!(f, "era end: slash {}, reward {}", slashings, rewards)
    }
}

impl ToBytes for EraEnd {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.equivocators.to_bytes()?);
        buffer.extend(self.rewards.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.equivocators.serialized_length() + self.rewards.serialized_length()
    }
}

impl FromBytes for EraEnd {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (equivocators, remainder) = Vec::<PublicKey>::from_bytes(bytes)?;
        let (rewards, remainder) = BTreeMap::<PublicKey, u64>::from_bytes(remainder)?;
        let era_end = EraEnd {
            equivocators,
            rewards,
        };
        Ok((era_end, remainder))
    }
}

/// The piece of information that will become the content of a future block after it was finalized
/// and before execution happened yet.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FinalizedBlock {
    proto_block: ProtoBlock,
    timestamp: Timestamp,
    era_end: Option<EraEnd>,
    era_id: EraId,
    height: u64,
    proposer: PublicKey,
}

impl FinalizedBlock {
    pub(crate) fn new(
        proto_block: ProtoBlock,
        timestamp: Timestamp,
        era_end: Option<EraEnd>,
        era_id: EraId,
        height: u64,
        proposer: PublicKey,
    ) -> Self {
        FinalizedBlock {
            proto_block,
            timestamp,
            era_end,
            era_id,
            height,
            proposer,
        }
    }

    /// The finalized proto block.
    pub(crate) fn proto_block(&self) -> &ProtoBlock {
        &self.proto_block
    }

    /// The timestamp from when the proto block was proposed.
    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns slashing and reward information if this is a switch block, i.e. the last block of
    /// its era.
    pub(crate) fn era_end(&self) -> Option<&EraEnd> {
        self.era_end.as_ref()
    }

    /// Returns the ID of the era this block belongs to.
    pub(crate) fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the height of this block.
    pub(crate) fn height(&self) -> u64 {
        self.height
    }

    /// Returns true if block is Genesis' child.
    /// Genesis child block is from era 0 and height 0.
    pub(crate) fn is_genesis_child(&self) -> bool {
        self.era_id() == EraId(0) && self.height() == 0
    }

    pub(crate) fn proposer(&self) -> PublicKey {
        self.proposer
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let deploy_count = rng.gen_range(0, 11);
        let deploy_hashes = iter::repeat_with(|| DeployHash::new(Digest::random(rng)))
            .take(deploy_count)
            .collect();
        let random_bit = rng.gen();
        let proto_block = ProtoBlock::new(deploy_hashes, random_bit);

        // TODO - make Timestamp deterministic.
        let timestamp = Timestamp::now();
        let era_end = if rng.gen_bool(0.1) {
            let equivocators_count = rng.gen_range(0, 5);
            let rewards_count = rng.gen_range(0, 5);
            Some(EraEnd {
                equivocators: iter::repeat_with(|| {
                    PublicKey::from(&SecretKey::new_ed25519(rng.gen()))
                })
                .take(equivocators_count)
                .collect(),
                rewards: iter::repeat_with(|| {
                    let pub_key = PublicKey::from(&SecretKey::new_ed25519(rng.gen()));
                    let reward = rng.gen_range(1, BLOCK_REWARD + 1);
                    (pub_key, reward)
                })
                .take(rewards_count)
                .collect(),
            })
        } else {
            None
        };
        let era = rng.gen_range(0, 5);
        let secret_key: SecretKey = SecretKey::new_ed25519(rng.gen());
        let public_key = PublicKey::from(&secret_key);

        FinalizedBlock::new(
            proto_block,
            timestamp,
            era_end,
            EraId(era),
            era * 10 + rng.gen_range(0, 10),
            public_key,
        )
    }
}

impl From<BlockHeader> for FinalizedBlock {
    fn from(header: BlockHeader) -> Self {
        let proto_block = ProtoBlock::new(header.deploy_hashes().clone(), header.random_bit);

        FinalizedBlock {
            proto_block,
            timestamp: header.timestamp,
            era_end: header.era_end,
            era_id: header.era_id,
            height: header.height,
            proposer: header.proposer,
        }
    }
}

impl Display for FinalizedBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finalized block {:10} in era {:?}, height {}, deploys {:10}, random bit {}, \
            timestamp {}",
            HexFmt(self.proto_block.hash().inner()),
            self.era_id,
            self.height,
            HexList(&self.proto_block.deploys),
            self.proto_block.random_bit,
            self.timestamp,
        )?;
        if let Some(ee) = &self.era_end {
            write!(formatter, ", era_end: {}", ee)?;
        }
        Ok(())
    }
}

/// A cryptographic hash identifying a [`Block`](struct.Block.html).
#[derive(
    Copy, Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug,
)]
pub struct BlockHash(Digest);

impl BlockHash {
    /// Constructs a new `BlockHash`.
    pub fn new(hash: Digest) -> Self {
        BlockHash(hash)
    }

    /// Returns the wrapped inner hash.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Creates a random block hash.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = Digest::random(rng);
        BlockHash(hash)
    }
}

impl Display for BlockHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "block-hash({})", self.0,)
    }
}

impl From<Digest> for BlockHash {
    fn from(digest: Digest) -> Self {
        Self(digest)
    }
}

impl AsRef<[u8]> for BlockHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl ToBytes for BlockHash {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for BlockHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = Digest::from_bytes(bytes)?;
        let block_hash = BlockHash(hash);
        Ok((block_hash, remainder))
    }
}

/// The header portion of a [`Block`](struct.Block.html).
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct BlockHeader {
    parent_hash: BlockHash,
    state_root_hash: Digest,
    body_hash: Digest,
    deploy_hashes: Vec<DeployHash>,
    random_bit: bool,
    accumulated_seed: Digest,
    era_end: Option<EraEnd>,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
    proposer: PublicKey,
}

impl BlockHeader {
    /// The parent block's hash.
    pub fn parent_hash(&self) -> &BlockHash {
        &self.parent_hash
    }

    /// The root hash of the resulting global state.
    pub fn state_root_hash(&self) -> &Digest {
        &self.state_root_hash
    }

    /// The hash of the block's body.
    pub fn body_hash(&self) -> &Digest {
        &self.body_hash
    }

    /// The list of deploy hashes included in the block.
    pub fn deploy_hashes(&self) -> &Vec<DeployHash> {
        &self.deploy_hashes
    }

    /// A random bit needed for initializing a future era.
    pub fn random_bit(&self) -> bool {
        self.random_bit
    }

    /// A seed needed for initializing a future era.
    pub fn accumulated_seed(&self) -> Digest {
        self.accumulated_seed
    }

    /// The timestamp from when the proto block was proposed.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns reward and slashing information if this is the era's last block.
    pub fn era_end(&self) -> Option<&EraEnd> {
        self.era_end.as_ref()
    }

    /// Returns `true` if this block is the last one in the current era.
    pub fn switch_block(&self) -> bool {
        self.era_end.is_some()
    }

    /// Era ID in which this block was created.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the height of this block, i.e. the number of ancestors.
    pub fn height(&self) -> u64 {
        self.height
    }

    /// Block proposer.
    pub fn proposer(&self) -> &PublicKey {
        &self.proposer
    }

    /// Returns true if block is Genesis' child.
    /// Genesis child block is from era 0 and height 0.
    pub(crate) fn is_genesis_child(&self) -> bool {
        self.era_id() == EraId(0) && self.height() == 0
    }

    // Serialize the block header.
    fn serialize(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.to_bytes()
    }

    /// Hash of the block header.
    pub fn hash(&self) -> BlockHash {
        let serialized_header = Self::serialize(&self)
            .unwrap_or_else(|error| panic!("should serialize block header: {}", error));
        BlockHash::new(hash::hash(&serialized_header))
    }
}

impl Display for BlockHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block header parent hash {}, post-state hash {}, body hash {}, deploys [{}], \
            random bit {}, accumulated seed {}, timestamp {}",
            self.parent_hash.inner(),
            self.state_root_hash,
            self.body_hash,
            DisplayIter::new(self.deploy_hashes.iter()),
            self.random_bit,
            self.accumulated_seed,
            self.timestamp,
        )?;
        if let Some(ee) = &self.era_end {
            write!(formatter, ", era_end: {}", ee)?;
        }
        Ok(())
    }
}

impl ToBytes for BlockHeader {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.parent_hash.to_bytes()?);
        buffer.extend(self.state_root_hash.to_bytes()?);
        buffer.extend(self.body_hash.to_bytes()?);
        buffer.extend(self.deploy_hashes.to_bytes()?);
        buffer.extend(self.random_bit.to_bytes()?);
        buffer.extend(self.accumulated_seed.to_bytes()?);
        buffer.extend(self.era_end.to_bytes()?);
        buffer.extend(self.timestamp.to_bytes()?);
        buffer.extend(self.era_id.to_bytes()?);
        buffer.extend(self.height.to_bytes()?);
        buffer.extend(self.proposer.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.parent_hash.serialized_length()
            + self.state_root_hash.serialized_length()
            + self.body_hash.serialized_length()
            + self.deploy_hashes.serialized_length()
            + self.random_bit.serialized_length()
            + self.accumulated_seed.serialized_length()
            + self.era_end.serialized_length()
            + self.timestamp.serialized_length()
            + self.era_id.serialized_length()
            + self.height.serialized_length()
            + self.proposer.serialized_length()
    }
}

impl FromBytes for BlockHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (parent_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (state_root_hash, remainder) = Digest::from_bytes(remainder)?;
        let (body_hash, remainder) = Digest::from_bytes(remainder)?;
        let (deploy_hashes, remainder) = Vec::<DeployHash>::from_bytes(remainder)?;
        let (random_bit, remainder) = bool::from_bytes(remainder)?;
        let (accumulated_seed, remainder) = Digest::from_bytes(remainder)?;
        let (era_end, remainder) = Option::<EraEnd>::from_bytes(remainder)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (era_id, remainder) = EraId::from_bytes(remainder)?;
        let (height, remainder) = u64::from_bytes(remainder)?;
        let (proposer, remainder) = PublicKey::from_bytes(remainder)?;
        let block_header = BlockHeader {
            parent_hash,
            state_root_hash,
            body_hash,
            deploy_hashes,
            random_bit,
            accumulated_seed,
            era_end,
            timestamp,
            era_id,
            height,
            proposer,
        };
        Ok((block_header, remainder))
    }
}

/// An error that can arise when validating a block's cryptographic integrity using its hashes
#[derive(Debug)]
pub enum BlockValidationError {
    /// Problem serializing some of a block's data into bytes
    SerializationError(bytesrepr::Error),

    /// The body hash in the header is not the same as the hash of the body of the block
    UnexpectedBodyHash {
        /// The block body hash specified in the header that is apparently incorrect
        expected_by_block_header: Digest,
        /// The actual hash of the block's body
        actual: Digest,
    },

    /// The block's hash is not the same as the header's hash
    UnexpectedBlockHash {
        /// The hash specified by the block
        expected_by_block: BlockHash,
        /// The actual hash of the block
        actual: BlockHash,
    },
}

impl Display for BlockValidationError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{:?}", self)
    }
}

impl From<bytesrepr::Error> for BlockValidationError {
    fn from(err: bytesrepr::Error) -> Self {
        BlockValidationError::SerializationError(err)
    }
}

/// A proto-block after execution, with the resulting post-state-hash.  This is the core component
/// of the Casper linear blockchain.
#[derive(DataSize, Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Block {
    hash: BlockHash,
    header: BlockHeader,
    body: (), // TODO: implement body of block
    proofs: Vec<Signature>,
}

impl Block {
    pub(crate) fn new(
        parent_hash: BlockHash,
        parent_seed: Digest,
        state_root_hash: Digest,
        finalized_block: FinalizedBlock,
    ) -> Self {
        let body = ();
        let serialized_body = Self::serialize_body(&body)
            .unwrap_or_else(|error| panic!("should serialize block body: {}", error));
        let body_hash = hash::hash(&serialized_body);

        let era_id = finalized_block.era_id();
        let height = finalized_block.height();

        let mut accumulated_seed = [0; Digest::LENGTH];

        let mut hasher = VarBlake2b::new(Digest::LENGTH).expect("should create hasher");
        hasher.update(parent_seed);
        hasher.update([finalized_block.proto_block.random_bit as u8]);
        hasher.finalize_variable(|slice| {
            accumulated_seed.copy_from_slice(slice);
        });

        let header = BlockHeader {
            parent_hash,
            state_root_hash,
            body_hash,
            deploy_hashes: finalized_block.proto_block.deploys,
            random_bit: finalized_block.proto_block.random_bit,
            accumulated_seed: accumulated_seed.into(),
            era_end: finalized_block.era_end,
            timestamp: finalized_block.timestamp,
            era_id,
            height,
            proposer: finalized_block.proposer,
        };

        let hash = header.hash();

        Block {
            hash,
            header,
            body,
            proofs: vec![],
        }
    }

    pub(crate) fn header(&self) -> &BlockHeader {
        &self.header
    }

    pub(crate) fn take_header(self) -> BlockHeader {
        self.header
    }

    /// The hash of this block's header.
    pub fn hash(&self) -> &BlockHash {
        &self.hash
    }

    pub(crate) fn state_root_hash(&self) -> &Digest {
        self.header.state_root_hash()
    }

    /// The deploy hashes included in this block.
    pub fn deploy_hashes(&self) -> &Vec<DeployHash> {
        self.header.deploy_hashes()
    }

    /// The height of a block.
    pub fn height(&self) -> u64 {
        self.header.height()
    }

    /// Appends the given signature to this block's proofs.  It should have been validated prior to
    /// this via `BlockHash::verify()`.
    pub(crate) fn append_proof(&mut self, proof: Signature) {
        self.proofs.push(proof)
    }

    /// Returns true if block already contains the proof.
    pub(crate) fn contains_proof(&self, proof: &Signature) -> bool {
        self.proofs.iter().any(|s| s == proof)
    }

    fn serialize_body(body: &()) -> Result<Vec<u8>, bytesrepr::Error> {
        body.to_bytes()
    }

    /// Check the integrity of a block by hashing its body and header
    pub fn verify(&self) -> Result<(), BlockValidationError> {
        let serialized_body = Block::serialize_body(&self.body)?;
        let actual_body_hash = hash::hash(&serialized_body);
        if self.header.body_hash != actual_body_hash {
            return Err(BlockValidationError::UnexpectedBodyHash {
                expected_by_block_header: self.header.body_hash,
                actual: actual_body_hash,
            });
        }
        let actual_header_hash = self.header.hash();
        if self.hash != actual_header_hash {
            return Err(BlockValidationError::UnexpectedBlockHash {
                expected_by_block: self.hash,
                actual: actual_header_hash,
            });
        }
        Ok(())
    }

    /// Overrides the height of a block.
    #[cfg(test)]
    pub fn set_height(&mut self, height: u64) -> &mut Self {
        self.header.height = height;
        self
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let parent_hash = BlockHash::new(Digest::random(rng));
        let state_root_hash = Digest::random(rng);
        let finalized_block = FinalizedBlock::random(rng);
        let parent_seed = Digest::random(rng);

        let mut block = Block::new(parent_hash, parent_seed, state_root_hash, finalized_block);

        let signatures_count = rng.gen_range(0, 11);
        for _ in 0..signatures_count {
            let secret_key = SecretKey::random(rng);
            let public_key = PublicKey::from(&secret_key);
            let signature = asymmetric_key::sign(block.hash.inner(), &secret_key, &public_key, rng);
            block.append_proof(signature);
        }

        block
    }
}

impl Display for Block {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "executed block {}, parent hash {}, post-state hash {}, body hash {}, deploys [{}], \
            random bit {}, timestamp {}, era_id {}, height {}, proofs count {}",
            self.hash.inner(),
            self.header.parent_hash.inner(),
            self.header.state_root_hash,
            self.header.body_hash,
            DisplayIter::new(self.header.deploy_hashes.iter()),
            self.header.random_bit,
            self.header.timestamp,
            self.header.era_id.0,
            self.header.height,
            self.proofs.len()
        )?;
        if let Some(ee) = &self.header.era_end {
            write!(formatter, ", era_end: {}", ee)?;
        }
        Ok(())
    }
}

impl ToBytes for Block {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.hash.to_bytes()?);
        buffer.extend(self.header.to_bytes()?);
        buffer.extend(self.proofs.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.hash.serialized_length()
            + self.header.serialized_length()
            + self.proofs.serialized_length()
    }
}

impl FromBytes for Block {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (header, remainder) = BlockHeader::from_bytes(remainder)?;
        let (proofs, remainder) = Vec::<Signature>::from_bytes(remainder)?;
        let block = Block {
            hash,
            header,
            body: (),
            proofs,
        };
        Ok((block, remainder))
    }
}

impl BlockLike for Block {
    fn deploys(&self) -> &Vec<DeployHash> {
        self.deploy_hashes()
    }
}

impl BlockLike for BlockHeader {
    fn deploys(&self) -> &Vec<DeployHash> {
        self.deploy_hashes()
    }
}

impl Item for Block {
    type Id = BlockHash;

    const TAG: Tag = Tag::Block;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn id(&self) -> Self::Id {
        *self.hash()
    }
}

/// A wrapper around `Block` for the purposes of fetching blocks by height in linear chain.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockByHeight {
    Absent(u64),
    Block(Box<Block>),
}

impl From<Block> for BlockByHeight {
    fn from(block: Block) -> Self {
        BlockByHeight::new(block)
    }
}

impl BlockByHeight {
    /// Creates a new `BlockByHeight`
    pub fn new(block: Block) -> Self {
        BlockByHeight::Block(Box::new(block))
    }

    pub fn height(&self) -> u64 {
        match self {
            BlockByHeight::Absent(height) => *height,
            BlockByHeight::Block(block) => block.height(),
        }
    }
}

impl Display for BlockByHeight {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockByHeight::Absent(height) => write!(f, "Block at height {} was absent.", height),
            BlockByHeight::Block(block) => {
                let hash: BlockHash = block.header().hash();
                write!(f, "Block at {} with hash {} found.", block.height(), hash)
            }
        }
    }
}

impl Item for BlockByHeight {
    type Id = u64;

    const TAG: Tag = Tag::BlockByHeight;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn id(&self) -> Self::Id {
        self.height()
    }
}

#[cfg(test)]
mod tests {
    use casper_types::bytesrepr;

    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn json_block_roundtrip() {
        let mut rng = crate::new_rng();
        let block = Block::random(&mut rng);
        let json_string = serde_json::to_string_pretty(&block).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(block, decoded);
    }

    #[test]
    fn json_finalized_block_roundtrip() {
        let mut rng = crate::new_rng();
        let finalized_block = FinalizedBlock::random(&mut rng);
        let json_string = serde_json::to_string_pretty(&finalized_block).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(finalized_block, decoded);
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
        let block = Block::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&block);
    }

    #[test]
    fn bytesrepr_roundtrip_era_end() {
        let mut rng = TestRng::new();
        let loop_iterations = 50;
        for _ in 0..loop_iterations {
            let finalized_block = FinalizedBlock::random(&mut rng);
            if let Some(era_end) = finalized_block.era_end() {
                bytesrepr::test_serialization_roundtrip(era_end);
            }
        }
    }

    #[test]
    fn random_block_check() {
        let mut rng = TestRng::from_seed([1u8; 16]);
        let loop_iterations = 50;
        for _ in 0..loop_iterations {
            Block::random(&mut rng)
                .verify()
                .expect("block hash should check");
        }
    }

    #[test]
    fn block_check_bad_body_hash_sad_path() {
        let mut rng = TestRng::from_seed([2u8; 16]);
        let mut block = Block::random(&mut rng);

        let bogus_block_hash = hash::hash(&[0xde, 0xad, 0xbe, 0xef]);
        block.header.body_hash = bogus_block_hash;

        let serialized_body =
            Block::serialize_body(&block.body).expect("Could not serialize block body");
        let actual_body_hash = hash::hash(&serialized_body);

        // No Eq trait for BlockValidationError, so pattern match
        match block.verify() {
            Err(BlockValidationError::UnexpectedBodyHash {
                expected_by_block_header,
                actual,
            }) if expected_by_block_header == bogus_block_hash && actual == actual_body_hash => {}
            unexpected => panic!("Bad check response: {:?}", unexpected),
        }
    }

    #[test]
    fn block_check_bad_block_hash_sad_path() {
        let mut rng = TestRng::from_seed([3u8; 16]);
        let mut block = Block::random(&mut rng);

        let bogus_block_hash: BlockHash = hash::hash(&[0xde, 0xad, 0xbe, 0xef]).into();
        block.hash = bogus_block_hash;

        let actual_block_hash = block.header.hash();

        // No Eq trait for BlockValidationError, so pattern match
        match block.verify() {
            Err(BlockValidationError::UnexpectedBlockHash {
                expected_by_block,
                actual,
            }) if expected_by_block == bogus_block_hash && actual == actual_block_hash => {}
            unexpected => panic!("Bad check response: {:?}", unexpected),
        }
    }
}
