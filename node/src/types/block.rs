// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

#[cfg(any(feature = "testing", test))]
use std::iter;
use std::{
    array::TryFromSliceError,
    collections::{BTreeMap, BTreeSet},
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;
use derive_more::Into;
use hex_fmt::HexList;
use once_cell::sync::Lazy;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_hashing::{ChunkWithProofVerificationError, Digest};
#[cfg(any(feature = "testing", test))]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, EraId, ProtocolVersion, PublicKey, SecretKey, Signature, Timestamp, U512,
};
#[cfg(any(feature = "testing", test))]
use casper_types::{crypto::generate_ed25519_keypair, system::auction::BLOCK_REWARD};
use tracing::{error, warn};

use crate::{
    components::consensus,
    rpcs::docs::DocExample,
    types::{
        error::{BlockCreationError, BlockValidationError},
        Approval, Deploy, DeployHash, DeployOrTransferHash, DeployWithApprovals, JsonBlock,
        JsonBlockHeader,
    },
    utils::DisplayIter,
};

use super::{Chunkable, Item, Tag, ValueOrChunk};
use crate::types::error::{
    BlockHeaderWithMetadataValidationError, BlockHeadersBatchValidationError,
    BlockWithMetadataValidationError,
};

static ERA_REPORT: Lazy<EraReport> = Lazy::new(|| {
    let secret_key_1 = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key_1 = PublicKey::from(&secret_key_1);
    let equivocators = vec![public_key_1];

    let secret_key_2 = SecretKey::ed25519_from_bytes([1; 32]).unwrap();
    let public_key_2 = PublicKey::from(&secret_key_2);
    let mut rewards = BTreeMap::new();
    rewards.insert(public_key_2, 1000);

    let secret_key_3 = SecretKey::ed25519_from_bytes([2; 32]).unwrap();
    let public_key_3 = PublicKey::from(&secret_key_3);
    let inactive_validators = vec![public_key_3];

    EraReport {
        equivocators,
        rewards,
        inactive_validators,
    }
});
static ERA_END: Lazy<EraEnd> = Lazy::new(|| {
    let secret_key_1 = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let public_key_1 = PublicKey::from(&secret_key_1);
    let next_era_validator_weights = {
        let mut next_era_validator_weights: BTreeMap<PublicKey, U512> = BTreeMap::new();
        next_era_validator_weights.insert(public_key_1, U512::from(123));
        next_era_validator_weights.insert(
            PublicKey::from(
                &SecretKey::ed25519_from_bytes([5u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            U512::from(456),
        );
        next_era_validator_weights.insert(
            PublicKey::from(
                &SecretKey::ed25519_from_bytes([6u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            U512::from(789),
        );
        next_era_validator_weights
    };

    let era_report = EraReport::doc_example().clone();
    EraEnd::new(era_report, next_era_validator_weights)
});
static FINALIZED_BLOCK: Lazy<FinalizedBlock> = Lazy::new(|| {
    let transfer_hashes = vec![*Deploy::doc_example().id()];
    let random_bit = true;
    let timestamp = *Timestamp::doc_example();
    let secret_key = SecretKey::doc_example();
    let public_key = PublicKey::from(secret_key);
    let block_payload = BlockPayload::new(
        vec![],
        transfer_hashes
            .into_iter()
            .map(|hash| {
                let approval = Approval::create(&hash, secret_key);
                let mut approvals = BTreeSet::new();
                approvals.insert(approval);
                DeployWithApprovals::new(hash, approvals)
            })
            .collect(),
        vec![],
        random_bit,
    );
    let era_report = Some(EraReport::doc_example().clone());
    let era_id = EraId::from(1);
    let height = 10;
    FinalizedBlock::new(
        block_payload,
        era_report,
        timestamp,
        era_id,
        height,
        public_key,
    )
});
static BLOCK: Lazy<Block> = Lazy::new(|| {
    let parent_hash = BlockHash::new(Digest::from([7u8; Digest::LENGTH]));
    let state_root_hash = Digest::from([8u8; Digest::LENGTH]);
    let finalized_block = FinalizedBlock::doc_example().clone();
    let parent_seed = Digest::from([9u8; Digest::LENGTH]);
    let protocol_version = ProtocolVersion::V1_0_0;

    let secret_key = SecretKey::doc_example();
    let public_key = PublicKey::from(secret_key);

    let next_era_validator_weights = {
        let mut next_era_validator_weights: BTreeMap<PublicKey, U512> = BTreeMap::new();
        next_era_validator_weights.insert(public_key, U512::from(123));
        next_era_validator_weights.insert(
            PublicKey::from(
                &SecretKey::ed25519_from_bytes([5u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            U512::from(456),
        );
        next_era_validator_weights.insert(
            PublicKey::from(
                &SecretKey::ed25519_from_bytes([6u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            U512::from(789),
        );
        Some(next_era_validator_weights)
    };

    Block::new(
        parent_hash,
        parent_seed,
        state_root_hash,
        finalized_block,
        next_era_validator_weights,
        protocol_version,
    )
    .expect("could not construct block")
});
static JSON_BLOCK: Lazy<JsonBlock> = Lazy::new(|| {
    let block = Block::doc_example().clone();
    let mut block_signature = BlockSignatures::new(*block.hash(), block.header().era_id);

    let secret_key = SecretKey::doc_example();
    let public_key = PublicKey::from(secret_key);

    let signature = crypto::sign(block.hash.inner(), secret_key, &public_key);
    block_signature.insert_proof(public_key, signature);

    JsonBlock::new(block, Some(block_signature))
});
static JSON_BLOCK_HEADER: Lazy<JsonBlockHeader> = Lazy::new(|| {
    let block_header = Block::doc_example().header().clone();
    JsonBlockHeader::from(block_header)
});

#[cfg(any(feature = "testing", test))]
const MAX_ERA_FOR_RANDOM_BLOCK: u64 = 6;

/// Error returned from constructing a `Block`.
#[derive(Debug, Error)]
pub enum Error {
    /// Error while encoding to JSON.
    #[error("encoding to JSON: {0}")]
    EncodeToJson(#[from] serde_json::Error),

    /// Error while decoding from JSON.
    #[error("decoding from JSON: {0}")]
    DecodeFromJson(Box<dyn StdError>),
}

impl From<base16::DecodeError> for Error {
    fn from(error: base16::DecodeError) -> Self {
        Error::DecodeFromJson(Box::new(error))
    }
}

impl From<TryFromSliceError> for Error {
    fn from(error: TryFromSliceError) -> Self {
        Error::DecodeFromJson(Box::new(error))
    }
}

/// The piece of information that will become the content of a future block (isn't finalized or
/// executed yet)
///
/// From the view of the consensus protocol this is the "consensus value": The protocol deals with
/// finalizing an order of `BlockPayload`s. Only after consensus has been reached, the block's
/// deploys actually get executed, and the executed block gets signed.
#[derive(
    Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Default,
)]
pub struct BlockPayload {
    deploys: Vec<DeployWithApprovals>,
    transfers: Vec<DeployWithApprovals>,
    accusations: Vec<PublicKey>,
    random_bit: bool,
}

impl BlockPayload {
    pub(crate) fn new(
        deploys: Vec<DeployWithApprovals>,
        transfers: Vec<DeployWithApprovals>,
        accusations: Vec<PublicKey>,
        random_bit: bool,
    ) -> Self {
        BlockPayload {
            deploys,
            transfers,
            accusations,
            random_bit,
        }
    }

    /// Returns the set of validators that are reported as faulty in this block.
    pub(crate) fn accusations(&self) -> &Vec<PublicKey> {
        &self.accusations
    }

    /// The list of deploys included in the block, excluding transfers.
    pub(crate) fn deploys(&self) -> &Vec<DeployWithApprovals> {
        &self.deploys
    }

    /// The list of transfers included in the block.
    pub(crate) fn transfers(&self) -> &Vec<DeployWithApprovals> {
        &self.transfers
    }

    /// An iterator over deploy hashes included in the block, excluding transfers.
    pub(crate) fn deploy_hashes(&self) -> impl Iterator<Item = &DeployHash> + Clone {
        self.deploys.iter().map(|dwa| dwa.deploy_hash())
    }

    /// An iterator over transfer hashes included in the block.
    pub(crate) fn transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> + Clone {
        self.transfers.iter().map(|dwa| dwa.deploy_hash())
    }

    /// Returns an iterator over all deploys and transfers.
    pub(crate) fn deploys_and_transfers_iter(
        &self,
    ) -> impl Iterator<Item = DeployOrTransferHash> + '_ {
        self.deploy_hashes()
            .copied()
            .map(DeployOrTransferHash::Deploy)
            .chain(
                self.transfer_hashes()
                    .copied()
                    .map(DeployOrTransferHash::Transfer),
            )
    }
}

impl Display for BlockPayload {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "block payload: deploys {}, transfers {}, accusations {:?}, random bit {}",
            HexList(self.deploy_hashes()),
            HexList(self.transfer_hashes()),
            self.accusations,
            self.random_bit,
        )
    }
}

#[cfg(any(feature = "testing", test))]
impl BlockPayload {
    #[allow(unused)] // TODO: remove when used in tests
    pub fn random(
        rng: &mut TestRng,
        num_deploys: usize,
        num_transfers: usize,
        num_approvals: usize,
        num_accusations: usize,
    ) -> Self {
        let mut total_approvals_left = num_approvals;
        const MAX_APPROVALS_PER_DEPLOY: usize = 100;

        let deploys = (0..num_deploys)
            .map(|n| {
                // We need at least one approval, and at least as many so that we are able to split
                // all the remaining approvals between the remaining deploys while not exceeding
                // the limit per deploy.
                let min_approval_count = total_approvals_left
                    .saturating_sub(
                        MAX_APPROVALS_PER_DEPLOY * (num_transfers + num_deploys - n - 1),
                    )
                    .max(1);
                // We have to leave at least one approval per deploy for the remaining deploys.
                let max_approval_count = MAX_APPROVALS_PER_DEPLOY
                    .min(total_approvals_left - (num_transfers + num_deploys - n - 1));
                let n_approvals = rng.gen_range(min_approval_count..=max_approval_count);
                total_approvals_left -= n_approvals;
                DeployWithApprovals::new(
                    DeployHash::random(rng),
                    (0..n_approvals).map(|_| Approval::random(rng)).collect(),
                )
            })
            .collect();

        let transfers = (0..num_transfers)
            .map(|n| {
                // We need at least one approval, and at least as many so that we are able to split
                // all the remaining approvals between the remaining transfers while not exceeding
                // the limit per deploy.
                let min_approval_count = total_approvals_left
                    .saturating_sub(MAX_APPROVALS_PER_DEPLOY * (num_transfers - n - 1))
                    .max(1);
                // We have to leave at least one approval per transfer for the remaining transfers.
                let max_approval_count =
                    MAX_APPROVALS_PER_DEPLOY.min(total_approvals_left - (num_transfers - n - 1));
                let n_approvals = rng.gen_range(min_approval_count..=max_approval_count);
                total_approvals_left -= n_approvals;
                DeployWithApprovals::new(
                    DeployHash::random(rng),
                    (0..n_approvals).map(|_| Approval::random(rng)).collect(),
                )
            })
            .collect();

        let accusations = (0..num_accusations)
            .map(|_| PublicKey::random(rng))
            .collect();

        Self {
            deploys,
            transfers,
            accusations,
            random_bit: rng.gen(),
        }
    }
}

/// Equivocation and reward information to be included in the terminal finalized block.
pub type EraReport = consensus::EraReport<PublicKey>;

impl Display for EraReport {
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

impl ToBytes for EraReport {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.equivocators.to_bytes()?);
        buffer.extend(self.rewards.to_bytes()?);
        buffer.extend(self.inactive_validators.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.equivocators.serialized_length()
            + self.rewards.serialized_length()
            + self.inactive_validators.serialized_length()
    }
}

impl FromBytes for EraReport {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (equivocators, remainder) = Vec::<PublicKey>::from_bytes(bytes)?;
        let (rewards, remainder) = BTreeMap::<PublicKey, u64>::from_bytes(remainder)?;
        let (inactive_validators, remainder) = Vec::<PublicKey>::from_bytes(remainder)?;

        let era_report = EraReport {
            equivocators,
            rewards,
            inactive_validators,
        };
        Ok((era_report, remainder))
    }
}

impl DocExample for EraReport {
    fn doc_example() -> &'static Self {
        &*ERA_REPORT
    }
}

/// The piece of information that will become the content of a future block after it was finalized
/// and before execution happened yet.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FinalizedBlock {
    deploy_hashes: Vec<DeployHash>,
    transfer_hashes: Vec<DeployHash>,
    timestamp: Timestamp,
    random_bit: bool,
    era_report: Box<Option<EraReport>>,
    era_id: EraId,
    height: u64,
    proposer: Box<PublicKey>,
}

impl FinalizedBlock {
    pub(crate) fn new(
        block_payload: BlockPayload,
        era_report: Option<EraReport>,
        timestamp: Timestamp,
        era_id: EraId,
        height: u64,
        proposer: PublicKey,
    ) -> Self {
        FinalizedBlock {
            deploy_hashes: block_payload.deploy_hashes().cloned().collect(),
            transfer_hashes: block_payload.transfer_hashes().cloned().collect(),
            timestamp,
            random_bit: block_payload.random_bit,
            era_report: Box::new(era_report),
            era_id,
            height,
            proposer: Box::new(proposer),
        }
    }

    /// The timestamp from when the block was proposed.
    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns slashing and reward information if this is a switch block, i.e. the last block of
    /// its era.
    pub(crate) fn era_report(&self) -> Option<&EraReport> {
        (*self.era_report).as_ref()
    }

    /// Returns the ID of the era this block belongs to.
    pub(crate) fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the height of this block.
    pub(crate) fn height(&self) -> u64 {
        self.height
    }

    pub(crate) fn proposer(&self) -> Box<PublicKey> {
        self.proposer.clone()
    }

    /// Returns the WebAssembly-deploy hashes for the finalized block. These correspond to complex
    /// smart contract operations that require a WebAssembly VM in the execution engine.
    pub(crate) fn deploy_hashes(&self) -> &[DeployHash] {
        &self.deploy_hashes
    }

    /// Returns the transfer hashes for the finalized block. These correspond to simple token
    /// transfers that do not require a VM as part of their execution.
    pub(crate) fn transfer_hashes(&self) -> &[DeployHash] {
        &self.transfer_hashes
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let era = rng.gen_range(0..5);
        let height = era * 10 + rng.gen_range(0..10);
        let is_switch = rng.gen_bool(0.1);

        FinalizedBlock::random_with_specifics(rng, EraId::from(era), height, is_switch, None)
    }

    #[cfg(any(feature = "testing", test))]
    /// Generates a random instance using a `TestRng`, but using the specified values.
    /// If `deploy` is `None`, random deploys will be generated, otherwise, the provided `deploy`
    /// will be used.
    pub fn random_with_specifics<'a, I: IntoIterator<Item = &'a Deploy>>(
        rng: &mut TestRng,
        era_id: EraId,
        height: u64,
        is_switch: bool,
        deploys_iter: I,
    ) -> Self {
        let mut deploys = deploys_iter
            .into_iter()
            .map(DeployWithApprovals::from)
            .collect::<Vec<_>>();
        if deploys.is_empty() {
            let count = rng.gen_range(0..11);
            deploys.extend(
                iter::repeat_with(|| DeployWithApprovals::from(&Deploy::random(rng))).take(count),
            );
        }
        let random_bit = rng.gen();
        // TODO - make Timestamp deterministic.
        let timestamp = Timestamp::now();
        let block_payload = BlockPayload::new(deploys, vec![], vec![], random_bit);

        let era_report = if is_switch {
            let equivocators_count = rng.gen_range(0..5);
            let rewards_count = rng.gen_range(0..5);
            let inactive_count = rng.gen_range(0..5);
            Some(EraReport {
                equivocators: iter::repeat_with(|| {
                    PublicKey::from(&SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap())
                })
                .take(equivocators_count)
                .collect(),
                rewards: iter::repeat_with(|| {
                    let pub_key = PublicKey::from(
                        &SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap(),
                    );
                    let reward = rng.gen_range(1..(BLOCK_REWARD + 1));
                    (pub_key, reward)
                })
                .take(rewards_count)
                .collect(),
                inactive_validators: iter::repeat_with(|| {
                    PublicKey::from(&SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap())
                })
                .take(inactive_count)
                .collect(),
            })
        } else {
            None
        };
        let secret_key: SecretKey = SecretKey::ed25519_from_bytes(rng.gen::<[u8; 32]>()).unwrap();
        let public_key = PublicKey::from(&secret_key);

        FinalizedBlock::new(
            block_payload,
            era_report,
            timestamp,
            era_id,
            height,
            public_key,
        )
    }
}

impl DocExample for FinalizedBlock {
    fn doc_example() -> &'static Self {
        &*FINALIZED_BLOCK
    }
}

impl From<Block> for FinalizedBlock {
    fn from(block: Block) -> Self {
        FinalizedBlock {
            deploy_hashes: block.body.deploy_hashes,
            transfer_hashes: block.body.transfer_hashes,
            timestamp: block.header.timestamp,
            random_bit: block.header.random_bit,
            era_report: Box::new(block.header.era_end.map(|era_end| era_end.era_report)),
            era_id: block.header.era_id,
            height: block.header.height,
            proposer: Box::new(block.body.proposer),
        }
    }
}

impl Display for FinalizedBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finalized block in era {:?}, height {}, deploys {:10}, transfers {:10}, \
            random bit {}, timestamp {}",
            self.era_id,
            self.height,
            HexList(&self.deploy_hashes),
            HexList(&self.transfer_hashes),
            self.random_bit,
            self.timestamp,
        )?;
        if let Some(ee) = *self.era_report.clone() {
            write!(formatter, ", era_end: {}", ee)?;
        }
        Ok(())
    }
}

/// A cryptographic hash identifying a [`Block`](struct.Block.html).
#[derive(
    Copy,
    Clone,
    DataSize,
    Default,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Debug,
    JsonSchema,
    Into,
)]
#[serde(deny_unknown_fields)]
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
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = rng.gen::<[u8; Digest::LENGTH]>().into();
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

/// Describes a block's hash and height.
#[derive(
    Clone, Copy, DataSize, Default, Eq, JsonSchema, Serialize, Deserialize, Debug, PartialEq,
)]
pub struct BlockHashAndHeight {
    /// The hash of the block.
    #[schemars(description = "The hash of this deploy's block.")]
    pub block_hash: BlockHash,
    /// The height of the block.
    #[schemars(description = "The height of this deploy's block.")]
    pub block_height: u64,
}

impl BlockHashAndHeight {
    pub fn new(block_hash: BlockHash, block_height: u64) -> Self {
        Self {
            block_hash,
            block_height,
        }
    }

    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        Self {
            block_hash: BlockHash::random(rng),
            block_height: rng.gen::<u64>(),
        }
    }
}

impl Display for BlockHashAndHeight {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "hash: {}, height {} ",
            self.block_hash, self.block_height
        )
    }
}

#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
/// A struct to contain information related to the end of an era and validator weights for the
/// following era.
pub struct EraEnd {
    /// Equivocation and reward information to be included in the terminal finalized block.
    era_report: EraReport,
    /// The validators for the upcoming era and their respective weights.
    next_era_validator_weights: BTreeMap<PublicKey, U512>,
}

impl EraEnd {
    fn new(era_report: EraReport, next_era_validator_weights: BTreeMap<PublicKey, U512>) -> Self {
        EraEnd {
            era_report,
            next_era_validator_weights,
        }
    }

    /// Equivocation and reward information to be included in the terminal finalized block.
    pub fn era_report(&self) -> &EraReport {
        &self.era_report
    }

    /// The validators for the upcoming era and their respective weights.
    pub fn next_era_validator_weights(&self) -> &BTreeMap<PublicKey, U512> {
        &self.next_era_validator_weights
    }
}

impl ToBytes for EraEnd {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.era_report.to_bytes()?);
        buffer.extend(self.next_era_validator_weights.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.era_report.serialized_length() + self.next_era_validator_weights.serialized_length()
    }
}

impl FromBytes for EraEnd {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (era_report, bytes) = EraReport::from_bytes(bytes)?;
        let (next_era_validator_weights, bytes) = BTreeMap::<PublicKey, U512>::from_bytes(bytes)?;
        let era_end = EraEnd {
            era_report,
            next_era_validator_weights,
        };
        Ok((era_end, bytes))
    }
}

impl Display for EraEnd {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "era_report: {} ", self.era_report)
    }
}

impl DocExample for EraEnd {
    fn doc_example() -> &'static Self {
        &*ERA_END
    }
}

/// The header portion of a [`Block`](struct.Block.html).
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct BlockHeader {
    parent_hash: BlockHash,
    state_root_hash: Digest,
    body_hash: Digest,
    random_bit: bool,
    /// The seed for the sequence of leaders accumulated from random_bits.
    accumulated_seed: Digest,
    era_end: Option<EraEnd>,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
    protocol_version: ProtocolVersion,
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

    /// A random bit needed for initializing a future era.
    pub fn random_bit(&self) -> bool {
        self.random_bit
    }

    /// A seed needed for initializing a future era.
    pub fn accumulated_seed(&self) -> Digest {
        self.accumulated_seed
    }

    /// Returns the `EraEnd` of a block if it is a switch block.
    pub fn era_end(&self) -> Option<&EraEnd> {
        self.era_end.as_ref()
    }

    /// The timestamp from when the block was proposed.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Era ID in which this block was created.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the era ID in which the next block would be created (that is, this block's era ID,
    /// or its successor if this is a switch block).
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

    /// The validators for the upcoming era and their respective weights (if this is a switch
    /// block).
    pub fn next_era_validator_weights(&self) -> Option<&BTreeMap<PublicKey, U512>> {
        match &self.era_end {
            Some(era_end) => {
                let validator_weights = era_end.next_era_validator_weights();
                Some(validator_weights)
            }
            None => None,
        }
    }

    /// Takes the validators for the upcoming era and their respective weights (if this is a switch
    /// block).
    pub fn maybe_take_next_era_validator_weights(self) -> Option<BTreeMap<PublicKey, U512>> {
        self.era_end
            .map(|era_end| era_end.next_era_validator_weights)
    }

    /// Hash of the block header.
    pub fn hash(&self) -> BlockHash {
        let serialized_header = Self::serialize(self)
            .unwrap_or_else(|error| panic!("should serialize block header: {}", error));
        BlockHash::new(Digest::hash(&serialized_header))
    }

    /// Returns true if block is Genesis' child.
    /// Genesis child block is from era 0 and height 0.
    pub(crate) fn is_genesis_child(&self) -> bool {
        self.era_id().is_genesis() && self.height() == 0
    }

    // Serialize the block header.
    fn serialize(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.to_bytes()
    }
}

impl Display for BlockHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block header parent hash {}, post-state hash {}, body hash {}, \
            random bit {}, accumulated seed {}, timestamp {}",
            self.parent_hash.inner(),
            self.state_root_hash,
            self.body_hash,
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
        buffer.extend(self.random_bit.to_bytes()?);
        buffer.extend(self.accumulated_seed.to_bytes()?);
        buffer.extend(self.era_end.to_bytes()?);
        buffer.extend(self.timestamp.to_bytes()?);
        buffer.extend(self.era_id.to_bytes()?);
        buffer.extend(self.height.to_bytes()?);
        buffer.extend(self.protocol_version.to_bytes()?);
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

impl FromBytes for BlockHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (parent_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (state_root_hash, remainder) = Digest::from_bytes(remainder)?;
        let (body_hash, remainder) = Digest::from_bytes(remainder)?;
        let (random_bit, remainder) = bool::from_bytes(remainder)?;
        let (accumulated_seed, remainder) = Digest::from_bytes(remainder)?;
        let (era_end, remainder) = Option::<EraEnd>::from_bytes(remainder)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (era_id, remainder) = EraId::from_bytes(remainder)?;
        let (height, remainder) = u64::from_bytes(remainder)?;
        let (protocol_version, remainder) = ProtocolVersion::from_bytes(remainder)?;
        let block_header = BlockHeader {
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
        };
        Ok((block_header, remainder))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct BlockHeaderWithMetadata {
    pub block_header: BlockHeader,
    pub block_signatures: BlockSignatures,
}

impl Display for BlockHeaderWithMetadata {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{} and {}", self.block_header, self.block_signatures)
    }
}

impl Item for BlockHeaderWithMetadata {
    type Id = u64;
    type ValidationError = BlockHeaderWithMetadataValidationError;
    const TAG: Tag = Tag::BlockHeaderAndFinalitySignaturesByHeight;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(&self) -> Result<(), Self::ValidationError> {
        validate_block_header_and_signature_hash(&self.block_header, &self.block_signatures)
    }

    fn id(&self) -> Self::Id {
        self.block_header.height()
    }
}

#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
/// ID identifying a request for a batch of block headers.
pub(crate) struct BlockHeadersBatchId {
    pub highest: u64,
    pub lowest: u64,
}

impl BlockHeadersBatchId {
    pub(crate) fn new(highest: u64, lowest: u64) -> Self {
        Self { highest, lowest }
    }

    pub(crate) fn from_known(lowest_known_block_header: &BlockHeader, max_batch_size: u64) -> Self {
        let highest = lowest_known_block_header.height().saturating_sub(1);
        let lowest = lowest_known_block_header
            .height()
            .saturating_sub(max_batch_size);

        Self { highest, lowest }
    }

    /// Return an iterator over block header heights starting from highest (inclusive) to lowest
    /// (inclusive).
    pub(crate) fn iter(&self) -> impl Iterator<Item = u64> {
        (self.lowest..=self.highest).rev()
    }

    /// Returns the length of the batch.
    pub(crate) fn len(&self) -> u64 {
        self.highest + 1 - self.lowest
    }
}

impl Display for BlockHeadersBatchId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "block header batch {}..={}", self.highest, self.lowest)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct BlockHeadersBatch(Vec<BlockHeader>);

impl BlockHeadersBatch {
    /// Validates whether received batch is:
    /// 1) highest block header from the batch has a hash is `latest_known.hash`
    /// 2) a link of header[n].parent == header[n+1].hash is maintained
    ///
    /// Returns lowest block header from the batch or error if batch fails validation.
    pub(crate) fn validate(
        &self,
        batch_id: &BlockHeadersBatchId,
        earliest_known: &BlockHeader,
    ) -> Result<BlockHeader, BlockHeadersBatchValidationError> {
        let highest_header = self
            .0
            .first()
            .ok_or(BlockHeadersBatchValidationError::BatchEmpty)?;

        if batch_id.len() != self.inner().len() as u64 {
            return Err(BlockHeadersBatchValidationError::IncorrectLength {
                expected: batch_id.len(),
                got: self.inner().len() as u64,
            });
        }

        // Check first header first b/c it's cheaper than verifying continuity.
        let highest_hash = highest_header.hash();
        if &highest_hash != earliest_known.parent_hash() {
            return Err(BlockHeadersBatchValidationError::HighestBlockHashMismatch {
                expected: *earliest_known.parent_hash(),
                got: highest_hash,
            });
        }

        self.0
            .last()
            .cloned()
            .ok_or(BlockHeadersBatchValidationError::BatchEmpty)
    }

    /// Tries to create an instance of `BlockHeadersBatch` from a `Vec<BlockHeader>`.
    ///
    /// Returns `Some(Self)` if data passes validation, otherwise `None`.
    pub(crate) fn from_vec(
        batch: Vec<BlockHeader>,
        requested_id: &BlockHeadersBatchId,
    ) -> Option<Self> {
        match batch.first() {
            Some(highest) => {
                if highest.height() != requested_id.highest {
                    error!(
                        expected_highest=?requested_id.highest,
                        got_highest=?highest,
                        "unexpected highest block header"
                    );
                    return None;
                }
            }
            None => {
                warn!("response cannot be an empty batch");
                return None;
            }
        }

        match batch.last() {
            Some(lowest) => {
                if lowest.height() != requested_id.lowest {
                    error!(
                        expected_lowest=?requested_id.lowest,
                        got_lowest=?lowest,
                        "unexpected lowest block header"
                    );
                    return None;
                }
            }
            None => {
                error!("input cannot be empty");
                return None;
            }
        }

        Some(Self(batch))
    }

    /// Returns inner value.
    pub(crate) fn into_inner(self) -> Vec<BlockHeader> {
        self.0
    }

    /// Returns a reference to an inner vector of block headers.
    pub(crate) fn inner(&self) -> &Vec<BlockHeader> {
        &self.0
    }

    /// Returns the lowest element from the batch.
    pub(crate) fn lowest(&self) -> Option<&BlockHeader> {
        self.0.last()
    }

    /// Tests whether the block header batch is continuous and in descending order.
    pub(crate) fn is_continuous_and_descending(batch: &[BlockHeader]) -> bool {
        batch
            .windows(2)
            .filter_map(|window| match &window {
                &[l, r] => Some((l, r)),
                _ => None,
            })
            .all(|(l, r)| l.height() == r.height() + 1 && l.parent_hash() == &r.hash())
    }

    #[cfg(test)]
    // Test-only constructor allowing creation of otherwise invalid data.
    fn new(batch: Vec<BlockHeader>) -> Self {
        Self(batch)
    }
}

impl Display for BlockHeadersBatch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "block header batch")
    }
}

impl Item for BlockHeadersBatch {
    type Id = BlockHeadersBatchId;

    type ValidationError = BlockHeadersBatchValidationError;

    const TAG: Tag = Tag::BlockHeaderBatch;

    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(&self) -> Result<(), Self::ValidationError> {
        if self.inner().is_empty() {
            return Err(BlockHeadersBatchValidationError::BatchEmpty);
        }

        if !BlockHeadersBatch::is_continuous_and_descending(self.inner()) {
            return Err(BlockHeadersBatchValidationError::BatchNotContinuous);
        }

        Ok(())
    }

    fn id(&self) -> Self::Id {
        let upper_batch_height = self.0.first().map(|h| h.height());
        let lower_batch_height = self.0.last().map(|h| h.height());
        if lower_batch_height.is_none() || upper_batch_height.is_none() {
            // ID should be infallible but it is possible that the `Vec` is empty.
            // In that case we log an error to indicate something went really wrong and use `(0,0)`.
            warn!(
                ?lower_batch_height,
                ?upper_batch_height,
                "received header batch is empty"
            );
        }
        BlockHeadersBatchId::new(
            upper_batch_height.unwrap_or_default(),
            lower_batch_height.unwrap_or_default(),
        )
    }
}

/// The body portion of a block.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct BlockBody {
    proposer: PublicKey,
    deploy_hashes: Vec<DeployHash>,
    transfer_hashes: Vec<DeployHash>,
}

impl BlockBody {
    /// The number of parts of a block body.
    pub const PARTS_COUNT: usize = 3;

    /// Creates a new body from deploy and transfer hashes.
    pub(crate) fn new(
        proposer: PublicKey,
        deploy_hashes: Vec<DeployHash>,
        transfer_hashes: Vec<DeployHash>,
    ) -> Self {
        BlockBody {
            proposer,
            deploy_hashes,
            transfer_hashes,
        }
    }

    /// Block proposer.
    pub fn proposer(&self) -> &PublicKey {
        &self.proposer
    }

    /// Retrieves the deploy hashes within the block.
    pub(crate) fn deploy_hashes(&self) -> &Vec<DeployHash> {
        &self.deploy_hashes
    }

    /// Retrieves the transfer hashes within the block.
    pub(crate) fn transfer_hashes(&self) -> &Vec<DeployHash> {
        &self.transfer_hashes
    }

    /// Returns deploy hashes of transactions in an order in which they were executed.
    pub(crate) fn transaction_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes()
            .iter()
            .chain(self.transfer_hashes().iter())
    }

    /// Computes the body hash by hashing the serialized bytes.
    pub fn hash(&self) -> Digest {
        let serialized_body = self
            .to_bytes()
            .unwrap_or_else(|error| panic!("should serialize block body: {}", error));
        Digest::hash(&serialized_body)
    }
}

impl Display for BlockBody {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{:?}", self)?;
        Ok(())
    }
}

impl ToBytes for BlockBody {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.proposer.to_bytes()?);
        buffer.extend(self.deploy_hashes.to_bytes()?);
        buffer.extend(self.transfer_hashes.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.proposer.serialized_length()
            + self.deploy_hashes.serialized_length()
            + self.transfer_hashes.serialized_length()
    }
}

impl FromBytes for BlockBody {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (proposer, bytes) = PublicKey::from_bytes(bytes)?;
        let (deploy_hashes, bytes) = Vec::<DeployHash>::from_bytes(bytes)?;
        let (transfer_hashes, bytes) = Vec::<DeployHash>::from_bytes(bytes)?;
        let body = BlockBody {
            proposer,
            deploy_hashes,
            transfer_hashes,
        };
        Ok((body, bytes))
    }
}

/// A storage representation of finality signatures with the associated block hash.
#[derive(Clone, Debug, PartialOrd, Ord, Hash, Serialize, Deserialize, DataSize, Eq, PartialEq)]
pub struct BlockSignatures {
    /// The block hash for a given block.
    pub(crate) block_hash: BlockHash,
    /// The era id for the given set of finality signatures.
    pub(crate) era_id: EraId,
    /// The signatures associated with the block hash.
    pub(crate) proofs: BTreeMap<PublicKey, Signature>,
}

impl BlockSignatures {
    pub(crate) fn new(block_hash: BlockHash, era_id: EraId) -> Self {
        BlockSignatures {
            block_hash,
            era_id,
            proofs: BTreeMap::new(),
        }
    }

    pub(crate) fn insert_proof(
        &mut self,
        public_key: PublicKey,
        signature: Signature,
    ) -> Option<Signature> {
        self.proofs.insert(public_key, signature)
    }

    pub(crate) fn has_proof(&self, public_key: &PublicKey) -> bool {
        self.proofs.contains_key(public_key)
    }

    /// Verify the signatures contained within.
    pub(crate) fn verify(&self) -> Result<(), crypto::Error> {
        for (public_key, signature) in self.proofs.iter() {
            let signature = FinalitySignature {
                block_hash: self.block_hash,
                era_id: self.era_id,
                signature: *signature,
                public_key: public_key.clone(),
            };
            signature.verify()?;
        }
        Ok(())
    }
}

impl Display for BlockSignatures {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block signatures for hash: {} in era_id: {} with {} proofs",
            self.block_hash,
            self.era_id,
            self.proofs.len()
        )
    }
}

impl Item for BlockSignatures {
    type Id = BlockHash;
    type ValidationError = crypto::Error;
    const TAG: Tag = Tag::FinalitySignaturesByHash;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(&self) -> Result<(), Self::ValidationError> {
        self.verify()
    }

    fn id(&self) -> Self::Id {
        self.block_hash
    }
}

/// A proto-block after execution, with the resulting post-state-hash.  This is the core component
/// of the Casper linear blockchain.
#[derive(DataSize, Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Block {
    hash: BlockHash,
    header: BlockHeader,
    body: BlockBody,
}

impl Block {
    pub(crate) fn new(
        parent_hash: BlockHash,
        parent_seed: Digest,
        state_root_hash: Digest,
        finalized_block: FinalizedBlock,
        next_era_validator_weights: Option<BTreeMap<PublicKey, U512>>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, BlockCreationError> {
        let body = BlockBody::new(
            *finalized_block.proposer,
            finalized_block.deploy_hashes,
            finalized_block.transfer_hashes,
        );

        let body_hash = body.hash();

        let era_end = match (*finalized_block.era_report, next_era_validator_weights) {
            (None, None) => None,
            (Some(era_report), Some(next_era_validator_weights)) => {
                Some(EraEnd::new(era_report, next_era_validator_weights))
            }
            (maybe_era_report, maybe_next_era_validator_weights) => {
                return Err(BlockCreationError::CouldNotCreateEraEnd {
                    maybe_era_report,
                    maybe_next_era_validator_weights,
                })
            }
        };

        let accumulated_seed = Digest::hash_pair(parent_seed, [finalized_block.random_bit as u8]);

        let header = BlockHeader {
            parent_hash,
            state_root_hash,
            body_hash,
            random_bit: finalized_block.random_bit,
            accumulated_seed,
            era_end,
            timestamp: finalized_block.timestamp,
            era_id: finalized_block.era_id,
            height: finalized_block.height,
            protocol_version,
        };

        Ok(Block {
            hash: header.hash(),
            header,
            body,
        })
    }

    pub(crate) fn new_from_header_and_body(
        header: BlockHeader,
        body: BlockBody,
    ) -> Result<Self, BlockValidationError> {
        let hash = header.hash();
        let block = Block { hash, header, body };
        block.verify()?;
        Ok(block)
    }

    pub(crate) fn body(&self) -> &BlockBody {
        &self.body
    }

    /// Returns the reference to the header.
    pub fn header(&self) -> &BlockHeader {
        &self.header
    }

    /// Returns the header, consuming the block.
    pub fn take_header(self) -> BlockHeader {
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
        self.body.deploy_hashes()
    }

    /// The list of transfer hashes included in the block.
    pub fn transfer_hashes(&self) -> &Vec<DeployHash> {
        self.body.transfer_hashes()
    }

    /// The height of a block.
    pub fn height(&self) -> u64 {
        self.header.height()
    }

    /// The protocol version of the block.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.header.protocol_version
    }

    /// Returns the hash of the parent block.
    /// If the block is the first block in the linear chain returns `None`.
    pub fn parent(&self) -> Option<&BlockHash> {
        if self.header.is_genesis_child() {
            None
        } else {
            Some(self.header.parent_hash())
        }
    }

    /// Returns the timestamp of the block.
    pub fn timestamp(&self) -> Timestamp {
        self.header.timestamp()
    }

    /// Check the integrity of a block by hashing its body and header
    pub fn verify(&self) -> Result<(), BlockValidationError> {
        let actual_block_header_hash = self.header().hash();
        if *self.hash() != actual_block_header_hash {
            return Err(BlockValidationError::UnexpectedBlockHash {
                block: Box::new(self.to_owned()),
                actual_block_header_hash,
            });
        }

        let actual_block_body_hash = self.body.hash();
        if self.header.body_hash != actual_block_body_hash {
            return Err(BlockValidationError::UnexpectedBodyHash {
                block: Box::new(self.to_owned()),
                actual_block_body_hash,
            });
        }

        Ok(())
    }

    /// Overrides the era end of a block with a `None`, making it a non-switch block.
    #[cfg(any(feature = "testing", test))]
    pub fn disable_switch_block(&mut self) -> &mut Self {
        let _ = self.header.era_end.take();
        self.hash = self.header.hash();
        self
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let era = rng.gen_range(0..MAX_ERA_FOR_RANDOM_BLOCK);
        let height = era * 10 + rng.gen_range(0..10);
        let is_switch = rng.gen_bool(0.1);

        Block::random_with_specifics(
            rng,
            EraId::from(era),
            height,
            ProtocolVersion::V1_0_0,
            is_switch,
            None,
        )
    }

    /// Generates a random instance using a `TestRng`, but using the specified values.
    #[cfg(any(feature = "testing", test))]
    pub fn random_with_specifics<'a, I: IntoIterator<Item = &'a Deploy>>(
        rng: &mut TestRng,
        era_id: EraId,
        height: u64,
        protocol_version: ProtocolVersion,
        is_switch: bool,
        deploys_iter: I,
    ) -> Self {
        let parent_hash = BlockHash::new(rng.gen::<[u8; Digest::LENGTH]>().into());
        let state_root_hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        let finalized_block =
            FinalizedBlock::random_with_specifics(rng, era_id, height, is_switch, deploys_iter);
        let parent_seed = rng.gen::<[u8; Digest::LENGTH]>().into();
        let next_era_validator_weights = finalized_block
            .clone()
            .era_report
            .map(|_| BTreeMap::<PublicKey, U512>::default());

        Block::new(
            parent_hash,
            parent_seed,
            state_root_hash,
            finalized_block,
            next_era_validator_weights,
            protocol_version,
        )
        .expect("Could not create random block with specifics")
    }
}

impl DocExample for Block {
    fn doc_example() -> &'static Self {
        &*BLOCK
    }
}

impl Display for Block {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "executed block {}, parent hash {}, post-state hash {}, body hash {}, \
             random bit {}, timestamp {}, era_id {}, height {}, protocol version: {}",
            self.hash.inner(),
            self.header.parent_hash.inner(),
            self.header.state_root_hash,
            self.header.body_hash,
            self.header.random_bit,
            self.header.timestamp,
            self.header.era_id.value(),
            self.header.height,
            self.header.protocol_version
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
        buffer.extend(self.body.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.hash.serialized_length()
            + self.header.serialized_length()
            + self.body.serialized_length()
    }
}

impl FromBytes for Block {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (header, remainder) = BlockHeader::from_bytes(remainder)?;
        let (body, remainder) = BlockBody::from_bytes(remainder)?;
        let block = Block { hash, header, body };
        Ok((block, remainder))
    }
}

impl Item for Block {
    type Id = BlockHash;
    type ValidationError = BlockValidationError;

    const TAG: Tag = Tag::Block;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(&self) -> Result<(), Self::ValidationError> {
        self.verify()
    }

    fn id(&self) -> Self::Id {
        *self.hash()
    }
}

/// A wrapper around `Block` for the purposes of fetching blocks by height in linear chain.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockWithMetadata {
    pub block: Block,
    pub block_signatures: BlockSignatures,
}

impl Display for BlockWithMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Block at {} with hash {} with {} block signatures.",
            self.block.height(),
            self.block.hash(),
            self.block_signatures.proofs.len()
        )
    }
}

fn validate_block_header_and_signature_hash(
    block_header: &BlockHeader,
    finality_signatures: &BlockSignatures,
) -> Result<(), BlockHeaderWithMetadataValidationError> {
    if block_header.hash() != finality_signatures.block_hash {
        return Err(
            BlockHeaderWithMetadataValidationError::FinalitySignaturesHaveUnexpectedBlockHash {
                expected_block_hash: block_header.hash(),
                finality_signatures_block_hash: finality_signatures.block_hash,
            },
        );
    }
    if block_header.era_id != finality_signatures.era_id {
        return Err(
            BlockHeaderWithMetadataValidationError::FinalitySignaturesHaveUnexpectedEraId {
                expected_era_id: block_header.era_id,
                finality_signatures_era_id: finality_signatures.era_id,
            },
        );
    }
    Ok(())
}

impl Item for BlockWithMetadata {
    type Id = u64;
    type ValidationError = BlockWithMetadataValidationError;

    const TAG: Tag = Tag::BlockAndMetadataByHeight;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(&self) -> Result<(), Self::ValidationError> {
        self.block.verify()?;
        validate_block_header_and_signature_hash(self.block.header(), &self.block_signatures)?;
        Ok(())
    }

    fn id(&self) -> Self::Id {
        self.block.height()
    }
}

#[derive(DataSize, Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Wrapper around block and its deploys.
pub struct BlockAndDeploys {
    /// Block part.
    pub block: Block,
    /// Deploys.
    pub deploys: Vec<Deploy>,
}

impl Display for BlockAndDeploys {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "block {} and deploys", self.block.hash())
    }
}

impl Item for BlockAndDeploys {
    type Id = BlockHash;

    type ValidationError = BlockValidationError;

    const TAG: Tag = Tag::BlockAndDeploysByHash;

    // false b/c we're not validating finality signatures.
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(&self) -> Result<(), Self::ValidationError> {
        self.block.verify()?;
        // Validate that we've got all of the deploys we should have gotten, and that their hashes
        // are valid.
        for deploy_hash in self
            .block
            .deploy_hashes()
            .iter()
            .chain(self.block.transfer_hashes().iter())
        {
            match self
                .deploys
                .iter()
                .find(|&deploy| deploy.id() == deploy_hash)
            {
                Some(deploy) => deploy.has_valid_hash().map_err(|error| {
                    BlockValidationError::UnexpectedDeployHash {
                        block: Box::new(self.block.clone()),
                        invalid_deploy: Box::new(deploy.clone()),
                        deploy_configuration_failure: error,
                    }
                })?,
                None => {
                    return Err(BlockValidationError::MissingDeploy {
                        block: Box::new(self.block.clone()),
                        missing_deploy: *deploy_hash,
                    })
                }
            }
        }

        // Check we got no extra deploys.
        let expected_deploys_count =
            self.block.deploy_hashes().len() + self.block.transfer_hashes().len();
        if expected_deploys_count < self.deploys.len() {
            return Err(BlockValidationError::ExtraDeploys {
                block: Box::new(self.block.clone()),
                extra_deploys_count: (self.deploys.len() - expected_deploys_count) as u32,
            });
        }

        Ok(())
    }

    fn id(&self) -> Self::Id {
        *self.block.hash()
    }
}

/// Represents block effects or chunk of complete value.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum BlockEffectsOrChunk {
    /// Represents legacy block effects.
    /// Legacy meaning their merkle root can't be verified against what is expected.
    BlockEffectsLegacy {
        /// Block to which this value or chunk refers to.
        block_hash: BlockHash,
        /// Complete block effects for the block or a chunk of the complete data.
        value: ValueOrChunk<Vec<casper_types::ExecutionResult>>,
    },
    /// Represents post-1.5.0 block effects.
    BlockEffects {
        /// Block to which this value or chunk refers to.
        block_hash: BlockHash,
        /// Complete block effects for the block or a chunk of the complete data.
        value: ValueOrChunk<Vec<casper_types::ExecutionResult>>,
    },
}

impl BlockEffectsOrChunk {
    /// Verifies equivalence of the effects (or chunks) merkle root hash with the expected value.
    pub fn validate(&self, expected_merkle_root: &Digest) -> Result<bool, bytesrepr::Error> {
        match self {
            // For "legacy" block effects we can't verify their correctness as there's no reference,
            // expected value to compare against.
            BlockEffectsOrChunk::BlockEffectsLegacy { .. } => Ok(true),
            BlockEffectsOrChunk::BlockEffects { value, .. } => match value {
                ValueOrChunk::Value(block_effects) => {
                    Ok(&Chunkable::hash(&block_effects)? == expected_merkle_root)
                }
                ValueOrChunk::ChunkWithProof(chunk_with_proof) => {
                    Ok(&chunk_with_proof.proof().root_hash() == expected_merkle_root)
                }
            },
        }
    }

    /// Consumes `self` and returns inner `ValueOrChunk` field.
    /// Throws away information about the context in which the request was made - legacy or not.
    pub fn flatten(self) -> ValueOrChunk<Vec<casper_types::ExecutionResult>> {
        match self {
            BlockEffectsOrChunk::BlockEffectsLegacy { value, .. }
            | BlockEffectsOrChunk::BlockEffects { value, .. } => value,
        }
    }
}

impl Display for BlockEffectsOrChunk {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockEffectsOrChunk::BlockEffectsLegacy {
                block_hash,
                value: _,
            } => {
                write!(
                    f,
                    "block effects (or chunk) for pre-1.5 block with hash={}",
                    block_hash
                )
            }
            BlockEffectsOrChunk::BlockEffects {
                block_hash,
                value: _,
            } => {
                write!(
                    f,
                    "block effects (or chunk) for post-1.5 block={}",
                    block_hash
                )
            }
        }
    }
}

/// ID of the request for block effects or chunk.
#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BlockEffectsOrChunkId {
    /// Request for pre-1.5 block's effects (or chunk).
    BlockEffectsOrChunkLegacyId {
        /// Index of the chunk being requested.
        chunk_index: u64,
        /// Hash of the block.
        block_hash: BlockHash,
    },
    /// Request for post-1.5 block's effects (or chunk).
    BlockEffectsOrChunkId {
        /// Index of the chunk being requested.
        chunk_index: u64,
        /// Hash of the block.
        block_hash: BlockHash,
    },
}

impl BlockEffectsOrChunkId {
    /// Returns an instance of post-1.5 request for block effects.
    /// The `chunk_index` is set to 0 as the starting point of the fetch cycle.
    /// If the effects are stored without chunking the index will be 0 as well.
    pub fn new(block_hash: BlockHash) -> Self {
        BlockEffectsOrChunkId::BlockEffectsOrChunkId {
            chunk_index: 0,
            block_hash,
        }
    }

    /// Constructs a request ID for legacy block effects - pre-1.5.0
    /// The `chunk_index` is set to 0 as the starting point of the fetch cycle.
    /// If the effects are stored without chunking the index will be 0 as well.
    pub fn legacy(block_hash: BlockHash) -> Self {
        BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId {
            chunk_index: 0,
            block_hash,
        }
    }

    /// Given a serialized ID, deserializes it for display purposes.
    fn fmt_serialized(f: &mut Formatter, serialized_id: &[u8]) -> fmt::Result {
        match bincode::deserialize::<Self>(serialized_id) {
            Ok(ref effects_or_chunk_id) => fmt::Display::fmt(effects_or_chunk_id, f),
            Err(_) => f.write_str("<invalid>"),
        }
    }

    /// Returns the request for the `next_chunk` retaining the original request's block hash.
    pub fn next_chunk(&self, next_chunk: u64) -> Self {
        match self {
            BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId { block_hash, .. } => {
                BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId {
                    chunk_index: next_chunk,
                    block_hash: *block_hash,
                }
            }
            BlockEffectsOrChunkId::BlockEffectsOrChunkId { block_hash, .. } => {
                BlockEffectsOrChunkId::BlockEffectsOrChunkId {
                    chunk_index: next_chunk,
                    block_hash: *block_hash,
                }
            }
        }
    }

    pub(crate) fn block_hash(&self) -> &BlockHash {
        match self {
            BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId { block_hash, .. }
            | BlockEffectsOrChunkId::BlockEffectsOrChunkId { block_hash, .. } => block_hash,
        }
    }

    pub(crate) fn chunk_index(&self) -> u64 {
        match self {
            BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId { chunk_index, .. }
            | BlockEffectsOrChunkId::BlockEffectsOrChunkId { chunk_index, .. } => *chunk_index,
        }
    }

    /// Constructs a response for the request, retaining the requests' variant and `block_hash`.
    pub(crate) fn response(
        &self,
        value: ValueOrChunk<Vec<casper_types::ExecutionResult>>,
    ) -> BlockEffectsOrChunk {
        match self {
            BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId { block_hash, .. } => {
                BlockEffectsOrChunk::BlockEffectsLegacy {
                    block_hash: *block_hash,
                    value,
                }
            }
            BlockEffectsOrChunkId::BlockEffectsOrChunkId { block_hash, .. } => {
                BlockEffectsOrChunk::BlockEffects {
                    block_hash: *block_hash,
                    value,
                }
            }
        }
    }
}

impl Display for BlockEffectsOrChunkId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId {
                chunk_index,
                block_hash,
            } => write!(
                f,
                "BlockEffectsOrChunkLegacyId({}, {})",
                chunk_index, block_hash
            ),
            BlockEffectsOrChunkId::BlockEffectsOrChunkId {
                chunk_index,
                block_hash,
            } => write!(f, "BlockEffectsOrChunk({}, {})", chunk_index, block_hash),
        }
    }
}

/// Helper struct to on-demand deserialize a trie or chunk ID for display purposes.
pub struct BlockEffectsOrChunkIdDisplay<'a>(pub &'a [u8]);

impl<'a> Display for BlockEffectsOrChunkIdDisplay<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        BlockEffectsOrChunkId::fmt_serialized(f, self.0)
    }
}

impl Item for BlockEffectsOrChunk {
    type Id = BlockEffectsOrChunkId;

    type ValidationError = ChunkWithProofVerificationError;

    const TAG: Tag = Tag::BlockEffects;

    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(&self) -> Result<(), Self::ValidationError> {
        match self {
            BlockEffectsOrChunk::BlockEffectsLegacy {
                block_hash: _,
                value,
            } => match value {
                ValueOrChunk::Value(_) => Ok(()),
                ValueOrChunk::ChunkWithProof(chunk_with_proof) => chunk_with_proof.verify(),
            },
            BlockEffectsOrChunk::BlockEffects {
                block_hash: _,
                value,
            } => match value {
                ValueOrChunk::Value(_) => Ok(()),
                ValueOrChunk::ChunkWithProof(chunk_with_proof) => chunk_with_proof.verify(),
            },
        }
    }

    fn id(&self) -> Self::Id {
        match self {
            BlockEffectsOrChunk::BlockEffectsLegacy { block_hash, value } => match value {
                ValueOrChunk::Value(_) => BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId {
                    chunk_index: 0,
                    block_hash: *block_hash,
                },
                ValueOrChunk::ChunkWithProof(chunks) => {
                    BlockEffectsOrChunkId::BlockEffectsOrChunkLegacyId {
                        chunk_index: chunks.proof().index(),
                        block_hash: *block_hash,
                    }
                }
            },
            // We can't calculate the correcntess of the data against the merkle root of the block
            // effects as they weren't part of the request's ID.
            BlockEffectsOrChunk::BlockEffects { block_hash, value } => match value {
                ValueOrChunk::Value(_) => BlockEffectsOrChunkId::BlockEffectsOrChunkId {
                    chunk_index: 0,
                    block_hash: *block_hash,
                },
                ValueOrChunk::ChunkWithProof(chunks) => {
                    BlockEffectsOrChunkId::BlockEffectsOrChunkId {
                        chunk_index: chunks.proof().index(),
                        block_hash: *block_hash,
                    }
                }
            },
        }
    }
}

pub(crate) mod json_compatibility {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, JsonSchema, Clone, PartialEq, Eq, DataSize)]
    #[serde(deny_unknown_fields)]
    struct Reward {
        validator: PublicKey,
        amount: u64,
    }

    #[derive(Serialize, Deserialize, Debug, JsonSchema, Clone, PartialEq, Eq, DataSize)]
    #[serde(deny_unknown_fields)]
    struct ValidatorWeight {
        validator: PublicKey,
        weight: U512,
    }

    /// Equivocation and reward information to be included in the terminal block.
    #[derive(Serialize, Deserialize, Debug, JsonSchema, Clone, PartialEq, Eq, DataSize)]
    #[serde(deny_unknown_fields)]
    struct JsonEraReport {
        equivocators: Vec<PublicKey>,
        rewards: Vec<Reward>,
        inactive_validators: Vec<PublicKey>,
    }

    impl From<EraReport> for JsonEraReport {
        fn from(era_report: EraReport) -> Self {
            JsonEraReport {
                equivocators: era_report.equivocators,
                rewards: era_report
                    .rewards
                    .into_iter()
                    .map(|(validator, amount)| Reward { validator, amount })
                    .collect(),
                inactive_validators: era_report.inactive_validators,
            }
        }
    }

    impl From<JsonEraReport> for EraReport {
        fn from(era_report: JsonEraReport) -> Self {
            let equivocators = era_report.equivocators;
            let rewards = era_report
                .rewards
                .into_iter()
                .map(|reward| (reward.validator, reward.amount))
                .collect();
            let inactive_validators = era_report.inactive_validators;
            EraReport {
                equivocators,
                rewards,
                inactive_validators,
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug, JsonSchema, Clone, PartialEq, Eq, DataSize)]
    #[serde(deny_unknown_fields)]
    pub struct JsonEraEnd {
        era_report: JsonEraReport,
        next_era_validator_weights: Vec<ValidatorWeight>,
    }

    impl From<EraEnd> for JsonEraEnd {
        fn from(data: EraEnd) -> Self {
            let json_era_end = JsonEraReport::from(data.era_report);
            let json_validator_weights = data
                .next_era_validator_weights
                .iter()
                .map(|(validator, weight)| ValidatorWeight {
                    validator: validator.clone(),
                    weight: *weight,
                })
                .collect();
            JsonEraEnd {
                era_report: json_era_end,
                next_era_validator_weights: json_validator_weights,
            }
        }
    }

    impl From<JsonEraEnd> for EraEnd {
        fn from(json_data: JsonEraEnd) -> Self {
            let era_report = EraReport::from(json_data.era_report);
            let validator_weights = json_data
                .next_era_validator_weights
                .iter()
                .map(|validator_weight| {
                    (validator_weight.validator.clone(), validator_weight.weight)
                })
                .collect();
            EraEnd::new(era_report, validator_weights)
        }
    }

    /// JSON representation of a block header.
    #[derive(Serialize, Deserialize, Debug, JsonSchema, Clone, PartialEq, Eq, DataSize)]
    #[serde(deny_unknown_fields)]
    pub struct JsonBlockHeader {
        /// The parent hash.
        pub parent_hash: BlockHash,
        /// The state root hash.
        pub state_root_hash: Digest,
        /// The body hash.
        pub body_hash: Digest,
        /// Randomness bit.
        pub random_bit: bool,
        /// Accumulated seed.
        pub accumulated_seed: Digest,
        /// The era end.
        pub era_end: Option<JsonEraEnd>,
        /// The block timestamp.
        pub timestamp: Timestamp,
        /// The block era id.
        pub era_id: EraId,
        /// The block height.
        pub height: u64,
        /// The protocol version.
        pub protocol_version: ProtocolVersion,
    }

    impl From<BlockHeader> for JsonBlockHeader {
        fn from(block_header: BlockHeader) -> Self {
            JsonBlockHeader {
                parent_hash: block_header.parent_hash,
                state_root_hash: block_header.state_root_hash,
                body_hash: block_header.body_hash,
                random_bit: block_header.random_bit,
                accumulated_seed: block_header.accumulated_seed,
                era_end: block_header.era_end.map(JsonEraEnd::from),
                timestamp: block_header.timestamp,
                era_id: block_header.era_id,
                height: block_header.height,
                protocol_version: block_header.protocol_version,
            }
        }
    }

    impl From<JsonBlockHeader> for BlockHeader {
        fn from(block_header: JsonBlockHeader) -> Self {
            BlockHeader {
                parent_hash: block_header.parent_hash,
                state_root_hash: block_header.state_root_hash,
                body_hash: block_header.body_hash,
                random_bit: block_header.random_bit,
                accumulated_seed: block_header.accumulated_seed,
                era_end: block_header.era_end.map(EraEnd::from),
                timestamp: block_header.timestamp,
                era_id: block_header.era_id,
                height: block_header.height,
                protocol_version: block_header.protocol_version,
            }
        }
    }

    impl DocExample for JsonBlockHeader {
        fn doc_example() -> &'static Self {
            &*JSON_BLOCK_HEADER
        }
    }

    /// A JSON-friendly representation of `Body`
    #[derive(Serialize, Deserialize, Debug, JsonSchema, Clone, PartialEq, Eq, DataSize)]
    #[serde(deny_unknown_fields)]
    pub struct JsonBlockBody {
        proposer: PublicKey,
        deploy_hashes: Vec<DeployHash>,
        transfer_hashes: Vec<DeployHash>,
    }

    impl From<BlockBody> for JsonBlockBody {
        fn from(body: BlockBody) -> Self {
            JsonBlockBody {
                proposer: body.proposer().clone(),
                deploy_hashes: body.deploy_hashes().clone(),
                transfer_hashes: body.transfer_hashes().clone(),
            }
        }
    }

    impl From<JsonBlockBody> for BlockBody {
        fn from(json_body: JsonBlockBody) -> Self {
            BlockBody {
                proposer: json_body.proposer,
                deploy_hashes: json_body.deploy_hashes,
                transfer_hashes: json_body.transfer_hashes,
            }
        }
    }

    /// A JSON-friendly representation of `Block`.
    #[derive(Serialize, Deserialize, Debug, JsonSchema, Clone, PartialEq, Eq, DataSize)]
    #[serde(deny_unknown_fields)]
    pub struct JsonBlock {
        /// `BlockHash`
        pub hash: BlockHash,
        /// JSON-friendly block header.
        pub header: JsonBlockHeader,
        /// JSON-friendly block body.
        pub body: JsonBlockBody,
        /// JSON-friendly list of proofs for this block.
        pub proofs: Vec<JsonProof>,
    }

    impl JsonBlock {
        /// Create a new JSON Block with a Linear chain block and its associated signatures.
        pub fn new(block: Block, maybe_signatures: Option<BlockSignatures>) -> Self {
            let hash = *block.hash();
            let header = JsonBlockHeader::from(block.header.clone());
            let body = JsonBlockBody::from(block.body);
            let proofs = maybe_signatures
                .map(|signatures| signatures.proofs.into_iter().map(JsonProof::from).collect())
                .unwrap_or_default();

            JsonBlock {
                hash,
                header,
                body,
                proofs,
            }
        }

        /// Returns the hashes of the `Deploy`s included in the `Block`.
        pub fn deploy_hashes(&self) -> &Vec<DeployHash> {
            &self.body.deploy_hashes
        }

        /// Returns the hashes of the transfer `Deploy`s included in the `Block`.
        pub fn transfer_hashes(&self) -> &Vec<DeployHash> {
            &self.body.transfer_hashes
        }
    }

    impl DocExample for JsonBlock {
        fn doc_example() -> &'static Self {
            &*JSON_BLOCK
        }
    }

    impl From<JsonBlock> for Block {
        fn from(block: JsonBlock) -> Self {
            Block {
                hash: block.hash,
                header: BlockHeader::from(block.header),
                body: BlockBody::from(block.body),
            }
        }
    }

    /// A JSON-friendly representation of a proof, i.e. a block's finality signature.
    #[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, PartialEq, Eq, DataSize)]
    #[serde(deny_unknown_fields)]
    pub struct JsonProof {
        public_key: PublicKey,
        signature: Signature,
    }

    impl From<(PublicKey, Signature)> for JsonProof {
        fn from((public_key, signature): (PublicKey, Signature)) -> JsonProof {
            JsonProof {
                public_key,
                signature,
            }
        }
    }

    impl From<JsonProof> for (PublicKey, Signature) {
        fn from(proof: JsonProof) -> (PublicKey, Signature) {
            (proof.public_key, proof.signature)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn block_json_roundtrip() {
            let mut rng = TestRng::new();
            let block: Block = Block::random(&mut rng);
            let empty_signatures = BlockSignatures::new(*block.hash(), block.header().era_id);
            let json_block = JsonBlock::new(block.clone(), Some(empty_signatures));
            let block_deserialized = Block::from(json_block);
            assert_eq!(block, block_deserialized);
        }
    }
}

/// A validator's signature of a block, to confirm it is finalized. Clients and joining nodes should
/// wait until the signers' combined weight exceeds their fault tolerance threshold before accepting
/// the block as finalized.
#[derive(Debug, Clone, Serialize, Deserialize, DataSize, PartialEq, Eq, JsonSchema)]
pub struct FinalitySignature {
    /// Hash of a block this signature is for.
    pub block_hash: BlockHash,
    /// Era in which the block was created in.
    pub era_id: EraId,
    /// Signature over the block hash.
    pub signature: Signature,
    /// Public key of the signing validator.
    pub public_key: PublicKey,
}

impl FinalitySignature {
    /// Create an instance of `FinalitySignature`.
    pub fn new(
        block_hash: BlockHash,
        era_id: EraId,
        secret_key: &SecretKey,
        public_key: PublicKey,
    ) -> Self {
        let mut bytes = block_hash.inner().into_vec();
        bytes.extend_from_slice(&era_id.to_le_bytes());
        let signature = crypto::sign(bytes, secret_key, &public_key);
        FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
        }
    }

    /// Verifies whether the signature is correct.
    pub fn verify(&self) -> Result<(), crypto::Error> {
        // NOTE: This needs to be in sync with the `new` constructor.
        let mut bytes = self.block_hash.inner().into_vec();
        bytes.extend_from_slice(&self.era_id.to_le_bytes());
        crypto::verify(bytes, &self.signature, &self.public_key)
    }

    /// Returns a random `FinalitySignature` for the provided `block_hash` and `era_id`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_for_block(block_hash: BlockHash, era_id: u64) -> Self {
        let (sec_key, pub_key) = generate_ed25519_keypair();
        FinalitySignature::new(block_hash, EraId::new(era_id), &sec_key, pub_key)
    }
}

impl Display for FinalitySignature {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "finality signature for block hash {}, from {}",
            &self.block_hash, &self.public_key
        )
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use casper_types::{bytesrepr, testing::TestRng};

    use super::*;

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
    fn block_bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
        let block = Block::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&block);
    }

    #[test]
    fn block_header_bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
        let block_header: BlockHeader = Block::random(&mut rng).header;
        bytesrepr::test_serialization_roundtrip(&block_header);
    }

    #[test]
    fn bytesrepr_roundtrip_era_report() {
        let mut rng = TestRng::new();
        let loop_iterations = 50;
        for _ in 0..loop_iterations {
            let finalized_block = FinalizedBlock::random(&mut rng);
            if let Some(era_report) = finalized_block.era_report() {
                bytesrepr::test_serialization_roundtrip(era_report);
            }
        }
    }

    #[test]
    fn bytesrepr_roundtrip_era_end() {
        let mut rng = TestRng::new();
        let loop_iterations = 50;
        for _ in 0..loop_iterations {
            let block = Block::random(&mut rng);
            if let Some(data) = block.header.era_end {
                bytesrepr::test_serialization_roundtrip(&data)
            }
        }
    }

    #[test]
    fn random_block_check() {
        let mut rng = TestRng::new();
        let loop_iterations = 50;
        for _ in 0..loop_iterations {
            let random_block = Block::random(&mut rng);
            random_block.verify().expect("block hash should check");
        }
    }

    #[test]
    fn block_check_bad_body_hash_sad_path() {
        let mut rng = TestRng::new();

        let mut random_block = Block::random(&mut rng);
        let bogus_block_body_hash = Digest::hash(&[0xde, 0xad, 0xbe, 0xef]);
        random_block.header.body_hash = bogus_block_body_hash;
        random_block.hash = random_block.header.hash();
        let bogus_block_hash = random_block.hash;

        match random_block.verify() {
            Err(BlockValidationError::UnexpectedBodyHash {
                block,
                actual_block_body_hash,
            }) if block.hash == bogus_block_hash
                && block.header.body_hash == bogus_block_body_hash
                && block.body.hash() == actual_block_body_hash => {}
            unexpected => panic!("Bad check response: {:?}", unexpected),
        }
    }

    #[test]
    fn block_check_bad_block_hash_sad_path() {
        let mut rng = TestRng::new();

        let mut random_block = Block::random(&mut rng);
        let bogus_block_hash: BlockHash = Digest::hash(&[0xde, 0xad, 0xbe, 0xef]).into();
        random_block.hash = bogus_block_hash;

        // No Eq trait for BlockValidationError, so pattern match
        match random_block.verify() {
            Err(BlockValidationError::UnexpectedBlockHash {
                block,
                actual_block_header_hash,
            }) if block.hash == bogus_block_hash
                && block.header.hash() == actual_block_header_hash => {}
            unexpected => panic!("Bad check response: {:?}", unexpected),
        }
    }

    #[test]
    fn finality_signature() {
        let mut rng = TestRng::new();
        let block = Block::random(&mut rng);
        // Signature should be over both block hash and era id.
        let (secret_key, public_key) = generate_ed25519_keypair();
        let secret_rc = Rc::new(secret_key);
        let era_id = EraId::from(1);
        let fs = FinalitySignature::new(*block.hash(), era_id, &secret_rc, public_key.clone());
        assert!(fs.verify().is_ok());
        let signature = fs.signature;
        // Verify that signature includes era id.
        let fs_manufactured = FinalitySignature {
            block_hash: *block.hash(),
            era_id: EraId::from(2),
            signature,
            public_key,
        };
        // Test should fail b/c `signature` is over `era_id=1` and here we're using `era_id=2`.
        assert!(fs_manufactured.verify().is_err());
    }

    #[test]
    fn good_block_and_deploys_should_validate() {
        let mut rng = TestRng::new();

        let deploys = iter::repeat_with(|| Deploy::random(&mut rng))
            .take(5)
            .collect::<Vec<_>>();
        let block = Block::random_with_specifics(
            &mut rng,
            EraId::new(1),
            2,
            ProtocolVersion::V1_0_0,
            false,
            deploys.iter(),
        );
        let block_and_deploys = BlockAndDeploys { block, deploys };

        block_and_deploys
            .validate()
            .unwrap_or_else(|error| panic!("expected to be valid: {:?}", error));
    }

    #[test]
    fn block_and_deploys_should_fail_to_validate_with_extra_deploy() {
        let mut rng = TestRng::new();

        // Create block including only the first set of deploys.
        let deploys = iter::repeat_with(|| Deploy::random(&mut rng))
            .take(5)
            .collect::<Vec<_>>();
        let block = Block::random_with_specifics(
            &mut rng,
            EraId::new(1),
            2,
            ProtocolVersion::V1_0_0,
            false,
            deploys.iter(),
        );

        // Put both sets of deploys in `BlockAndDeploys`
        let extra_deploys = iter::repeat_with(|| Deploy::random(&mut rng))
            .take(3)
            .collect::<Vec<_>>();
        let block_and_deploys = BlockAndDeploys {
            block,
            deploys: deploys
                .iter()
                .chain(extra_deploys.iter())
                .cloned()
                .collect(),
        };

        match block_and_deploys.validate().unwrap_err() {
            BlockValidationError::ExtraDeploys {
                extra_deploys_count,
                ..
            } => {
                assert_eq!(extra_deploys_count, extra_deploys.len() as u32);
            }
            _ => panic!("should report extra deploys"),
        }
    }

    #[test]
    fn block_and_deploys_should_fail_to_validate_with_missing_deploy() {
        let mut rng = TestRng::new();

        // Create block including both sets of deploys.
        let deploys1 = iter::repeat_with(|| Deploy::random(&mut rng))
            .take(3)
            .collect::<Vec<_>>();
        let deploys2 = iter::repeat_with(|| Deploy::random(&mut rng))
            .take(2)
            .collect::<Vec<_>>();
        let block = Block::random_with_specifics(
            &mut rng,
            EraId::new(1),
            2,
            ProtocolVersion::V1_0_0,
            false,
            deploys1.iter().chain(deploys2.iter()),
        );

        // Only put first set of deploys in `BlockAndDeploys`
        let block_and_deploys = BlockAndDeploys {
            block,
            deploys: deploys1,
        };

        match block_and_deploys.validate().unwrap_err() {
            BlockValidationError::MissingDeploy { missing_deploy, .. } => {
                assert!(deploys2.iter().any(|deploy| *deploy.id() == missing_deploy))
            }
            _ => panic!("should report missing deploy"),
        };
    }

    #[test]
    fn block_and_deploys_should_fail_to_validate_with_bad_block() {
        let mut rng = TestRng::new();

        let deploys = vec![Deploy::random(&mut rng)];
        let mut block = Block::random_with_specifics(
            &mut rng,
            EraId::new(1),
            2,
            ProtocolVersion::V1_0_0,
            false,
            deploys.iter(),
        );

        // Invalidate the block.
        block.hash = BlockHash::random(&mut rng);

        let block_and_deploys = BlockAndDeploys { block, deploys };

        assert!(matches!(
            block_and_deploys.validate().unwrap_err(),
            BlockValidationError::UnexpectedBlockHash { .. }
        ));
    }

    #[test]
    fn block_and_deploys_should_fail_to_validate_with_bad_deploy() {
        let mut rng = TestRng::new();

        // Create an invalid deploy and include in deploy set.
        let mut bad_deploy = Deploy::random(&mut rng);
        bad_deploy.invalidate();
        let deploys = iter::repeat_with(|| Deploy::random(&mut rng))
            .take(5)
            .chain(iter::once(bad_deploy.clone()))
            .collect::<Vec<_>>();

        let block = Block::random_with_specifics(
            &mut rng,
            EraId::new(1),
            2,
            ProtocolVersion::V1_0_0,
            false,
            deploys.iter(),
        );

        let block_and_deploys = BlockAndDeploys { block, deploys };

        match block_and_deploys.validate().unwrap_err() {
            BlockValidationError::UnexpectedDeployHash { invalid_deploy, .. } => {
                assert_eq!(*invalid_deploy, bad_deploy);
            }
            _ => panic!("should report missing deploy"),
        };
    }

    #[test]
    fn block_headers_batch_id_iter() {
        let id = BlockHeadersBatchId::new(5, 1);
        assert_eq!(
            vec![5u64, 4, 3, 2, 1],
            id.iter().collect::<Vec<_>>(),
            ".iter() must return descending order"
        );

        let id = BlockHeadersBatchId::new(5, 5);
        assert_eq!(vec![5u64], id.iter().collect::<Vec<_>>());
    }

    #[test]
    fn block_headers_batch_id_len() {
        let id = BlockHeadersBatchId::new(5, 1);
        assert_eq!(id.len(), 5);
        let id = BlockHeadersBatchId::new(5, 5);
        assert_eq!(id.len(), 1);
    }

    #[test]
    fn block_headers_batch_id_from_known() {
        let mut rng = TestRng::new();
        let trusted_block: Block = Block::random_with_specifics(
            &mut rng,
            EraId::new(1),
            100,
            ProtocolVersion::V1_0_0,
            false,
            iter::empty(),
        );
        let trusted_header = trusted_block.take_header();

        let batch_size = 10;

        let id = BlockHeadersBatchId::from_known(&trusted_header, batch_size);

        assert_eq!(
            BlockHeadersBatchId::new(99, 90),
            id,
            "expected batch of proper length and skipping trusted height"
        );

        let id_saturated = BlockHeadersBatchId::from_known(&trusted_header, 1000);
        assert_eq!(
            BlockHeadersBatchId::new(99, 0),
            id_saturated,
            "expect batch towards Genesis"
        );

        let trusted_last = Block::random_with_specifics(
            &mut rng,
            EraId::new(0),
            1,
            ProtocolVersion::V1_0_0,
            false,
            iter::empty(),
        );

        let trusted_last_header = trusted_last.take_header();

        let id_last = BlockHeadersBatchId::from_known(&trusted_last_header, batch_size);
        assert_eq!(
            BlockHeadersBatchId::new(0, 0),
            id_last,
            "expected batch that saturates towards Genesis and doesn't include known height"
        );
    }

    // Utility struct that can be turned into an iterator that generates
    // continuous and descending blocks (i.e. blocks that have consecutive height
    // and parent hashes are correctly set). The height of the first block
    // in a series is choosen randomly.
    //
    // Additionally, this struct allows to generate switch blocks at a specific location in the
    // chain, for example: Setting `switch_block_indices` to [1; 3] and generating 5 blocks will
    // cause the 2nd and 4th blocks to be switch blocks.
    struct TestBlockSpec {
        block: Block,
        rng: TestRng,
        switch_block_indices: Option<Vec<u64>>,
    }

    impl TestBlockSpec {
        fn new(test_rng: TestRng, switch_block_indices: Option<Vec<u64>>) -> Self {
            let mut rng = test_rng;
            let block = Block::random(&mut rng);
            Self {
                block,
                rng,
                switch_block_indices,
            }
        }

        fn into_iter(self) -> TestBlockIterator {
            let block_height = self.block.height();
            TestBlockIterator {
                block: self.block,
                rng: self.rng,
                switch_block_indices: self.switch_block_indices.map(|switch_block_indices| {
                    switch_block_indices
                        .iter()
                        .map(|index| index + block_height)
                        .collect()
                }),
            }
        }
    }

    struct TestBlockIterator {
        block: Block,
        rng: TestRng,
        switch_block_indices: Option<Vec<u64>>,
    }

    impl Iterator for TestBlockIterator {
        type Item = Block;

        fn next(&mut self) -> Option<Self::Item> {
            let (is_switch_block, validators) = match &self.switch_block_indices {
                Some(switch_block_indices)
                    if switch_block_indices.contains(&self.block.height()) =>
                {
                    let secret_keys: Vec<SecretKey> = iter::repeat_with(|| {
                        SecretKey::ed25519_from_bytes(
                            self.rng.gen::<[u8; SecretKey::ED25519_LENGTH]>(),
                        )
                        .unwrap()
                    })
                    .take(4)
                    .collect();
                    let validators: BTreeMap<_, _> = secret_keys
                        .iter()
                        .map(|sk| (PublicKey::from(sk), 100.into()))
                        .collect();

                    (true, Some(validators))
                }
                Some(_) | None => (false, None),
            };

            let next = Block::new(
                self.block.id(),
                self.block.header().accumulated_seed(),
                *self.block.header().state_root_hash(),
                FinalizedBlock::random_with_specifics(
                    &mut self.rng,
                    self.block.header().era_id(),
                    self.block.header().height() + 1,
                    is_switch_block,
                    iter::empty(),
                ),
                validators,
                self.block.header().protocol_version(),
            )
            .unwrap();
            self.block = next.clone();
            Some(next)
        }
    }

    #[test]
    fn test_block_iter() {
        let rng = TestRng::new();
        let test_block = TestBlockSpec::new(rng, None);
        let mut block_batch = test_block.into_iter().take(100);
        let mut parent_block: Block = block_batch.next().unwrap();
        for current_block in block_batch {
            assert_eq!(
                current_block.header().height(),
                parent_block.header().height() + 1,
                "height should grow monotonically"
            );
            assert_eq!(
                current_block.header().parent_hash(),
                &parent_block.id(),
                "block's parent should point at previous block"
            );
            parent_block = current_block;
        }
    }

    #[test]
    fn test_block_iter_creates_switch_blocks() {
        let switch_block_indices = vec![0, 10, 76];

        let rng = TestRng::new();
        let test_block = TestBlockSpec::new(rng, Some(switch_block_indices.clone()));
        let block_batch: Vec<_> = test_block.into_iter().take(100).collect();

        let base_height = block_batch.first().expect("should have block").height();

        for block in block_batch {
            if switch_block_indices
                .iter()
                .map(|index| index + base_height)
                .any(|index| index == block.height())
            {
                assert!(block.header().is_switch_block())
            } else {
                assert!(!block.header().is_switch_block())
            }
        }
    }

    #[test]
    fn block_batch_is_continuous_and_descending() {
        let rng = TestRng::new();
        let test_block = TestBlockSpec::new(rng, None);

        let mut test_block_iter = test_block.into_iter();

        let mut batch = test_block_iter
            .by_ref()
            .take(3)
            .map(|block| block.take_header())
            .collect::<Vec<_>>();

        assert!(
            !BlockHeadersBatch::is_continuous_and_descending(batch.as_slice(),),
            "should fail b/c not descending"
        );

        batch.reverse();
        assert!(BlockHeadersBatch::is_continuous_and_descending(
            batch.as_slice(),
        ));

        let next_header = test_block_iter.next().unwrap().take_header();
        assert!(
            BlockHeadersBatch::is_continuous_and_descending(&[next_header]),
            "single block is valid batch"
        );

        let mut batch_with_holes = vec![test_block_iter.next().unwrap().take_header()];
        // Skip one block header
        let _ = test_block_iter.next().unwrap();
        batch_with_holes.push(test_block_iter.next().unwrap().take_header());

        assert!(!BlockHeadersBatch::is_continuous_and_descending(
            batch_with_holes.as_slice(),
        ));
    }

    #[test]
    fn block_headers_batch_from_vec() {
        let rng = TestRng::new();
        let test_block = TestBlockSpec::new(rng, None);

        let mut test_block_iter = test_block.into_iter();
        let mut batch = test_block_iter
            .by_ref()
            .take(3)
            .map(|block| block.take_header())
            .collect::<Vec<_>>();
        batch.reverse();

        let id = BlockHeadersBatchId::new(batch[0].height(), batch[2].height());

        let block_headers_batch = BlockHeadersBatch::from_vec(batch.clone(), &id);
        assert!(block_headers_batch.is_some());
        assert_eq!(block_headers_batch.unwrap().inner(), &batch);

        let missing_highest_batch = batch.clone().into_iter().skip(1).collect::<Vec<_>>();
        assert!(BlockHeadersBatch::from_vec(missing_highest_batch, &id).is_none());

        let missing_lowest_batch = batch
            .clone()
            .into_iter()
            .rev()
            .skip(1)
            .rev()
            .collect::<Vec<_>>();
        assert!(BlockHeadersBatch::from_vec(missing_lowest_batch, &id).is_none());
    }

    #[test]
    fn block_headers_batch_item_validate() {
        let empty_batch = BlockHeadersBatch::new(vec![]);
        assert_eq!(
            Item::validate(&empty_batch),
            Err(BlockHeadersBatchValidationError::BatchEmpty)
        );

        let rng = TestRng::new();
        let test_block = TestBlockSpec::new(rng, None);

        let mut test_block_iter = test_block.into_iter();

        // Invalid ordering.
        let invalid_batch = test_block_iter
            .by_ref()
            .take(3)
            .map(|block| block.take_header())
            .collect::<Vec<_>>();

        assert_eq!(
            Item::validate(&BlockHeadersBatch::new(invalid_batch.clone()),),
            Err(BlockHeadersBatchValidationError::BatchNotContinuous)
        );

        let valid_batch = {
            let mut tmp = invalid_batch;
            tmp.reverse();
            tmp
        };

        assert_eq!(
            Item::validate(&BlockHeadersBatch::new(valid_batch.clone()),),
            Ok(())
        );

        let single_el_valid = vec![valid_batch[0].clone()];
        assert_eq!(
            Item::validate(&BlockHeadersBatch::new(single_el_valid),),
            Ok(())
        );
    }

    #[test]
    fn block_headers_batch_validate() {
        let rng = TestRng::new();
        let test_block = TestBlockSpec::new(rng, None);

        let mut test_block_iter = test_block.into_iter();

        let headers = {
            let mut tmp_batch = test_block_iter
                .by_ref()
                .take(5)
                .map(|block| block.take_header())
                .collect::<Vec<_>>();
            tmp_batch.reverse();
            tmp_batch
        };

        let lowest = headers.last().cloned().unwrap();
        let (trusted, batch) = (
            headers.first().cloned().unwrap(),
            BlockHeadersBatch::new(headers[1..].to_vec()),
        );

        let batch_id = BlockHeadersBatchId::new(
            trusted.height() - 1,
            batch.inner().last().cloned().unwrap().height(),
        );

        assert_eq!(
            Ok(lowest),
            BlockHeadersBatch::validate(&batch, &batch_id, &trusted)
        );

        assert_eq!(
            Err(BlockHeadersBatchValidationError::BatchEmpty),
            BlockHeadersBatch::validate(&BlockHeadersBatch::new(vec![]), &batch_id, &trusted,)
        );

        let invalid_length_batch = BlockHeadersBatch::new(batch.inner().clone()[1..].to_vec());

        assert_eq!(
            Err(BlockHeadersBatchValidationError::IncorrectLength {
                expected: 4,
                got: 3
            }),
            BlockHeadersBatch::validate(&invalid_length_batch, &batch_id, &trusted,)
        );

        let (new_highest, invalid_highest_batch) = {
            let mut tmp = batch.inner().clone();
            tmp.reverse();
            (tmp.first().cloned().unwrap(), BlockHeadersBatch::new(tmp))
        };

        assert_eq!(
            Err(BlockHeadersBatchValidationError::HighestBlockHashMismatch {
                expected: batch.inner().first().cloned().unwrap().id(),
                got: new_highest.id()
            }),
            BlockHeadersBatch::validate(&invalid_highest_batch, &batch_id, &trusted,)
        );
    }
}
