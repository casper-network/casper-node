// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

mod approvals_hashes;

use std::{
    array::TryFromSliceError,
    cmp::{Ord, Ordering, PartialOrd},
    collections::{BTreeMap, BTreeSet},
    convert::Infallible,
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
};

use datasize::DataSize;
use derive_more::Into;
use itertools::Itertools;
use once_cell::sync::{Lazy, OnceCell};
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

use casper_hashing::{ChunkWithProofVerificationError, Digest};
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, EraId, ProtocolVersion, PublicKey, SecretKey, Signature, Timestamp, U512,
};
#[cfg(any(feature = "testing", test))]
use casper_types::{
    crypto::generate_ed25519_keypair, system::auction::BLOCK_REWARD, testing::TestRng,
};

use crate::{
    components::{block_synchronizer::ExecutionResultsChecksum, consensus},
    effect::GossipTarget,
    rpcs::docs::DocExample,
    types::{
        error::{BlockCreationError, BlockHeaderWithMetadataValidationError, BlockValidationError},
        Approval, Chunkable, Deploy, DeployHash, DeployHashWithApprovals, DeployId,
        DeployOrTransferHash, EmptyValidationMetadata, FetcherItem, GossiperItem, Item, JsonBlock,
        JsonBlockHeader, Tag, ValueOrChunk,
    },
    utils::{ds, DisplayIter},
};
pub(crate) use approvals_hashes::ApprovalsHashes;

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
    let transfer_hashes = vec![*Deploy::doc_example().hash()];
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
                DeployHashWithApprovals::new(hash, approvals)
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
pub(crate) struct BlockPayload {
    deploys: Vec<DeployHashWithApprovals>,
    transfers: Vec<DeployHashWithApprovals>,
    accusations: Vec<PublicKey>,
    random_bit: bool,
}

impl BlockPayload {
    pub(crate) fn new(
        deploys: Vec<DeployHashWithApprovals>,
        transfers: Vec<DeployHashWithApprovals>,
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
    pub(crate) fn deploys(&self) -> &Vec<DeployHashWithApprovals> {
        &self.deploys
    }

    /// The list of transfers included in the block.
    pub(crate) fn transfers(&self) -> &Vec<DeployHashWithApprovals> {
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

    /// The list of deploy hashes chained with the list of transfer hashes.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes().chain(self.transfer_hashes())
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
            "block payload: {} deploys, {} transfers, random bit {}, accusations {}",
            self.deploys.len(),
            self.transfers.len(),
            self.random_bit,
            DisplayIter::new(&self.accusations),
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
                DeployHashWithApprovals::new(
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
                DeployHashWithApprovals::new(
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

    /// The list of deploy hashes chained with the list of transfer hashes.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes.iter().chain(&self.transfer_hashes)
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
        use std::iter;

        let mut deploys = deploys_iter
            .into_iter()
            .map(DeployHashWithApprovals::from)
            .collect::<Vec<_>>();
        if deploys.is_empty() {
            let count = rng.gen_range(0..11);
            deploys.extend(
                iter::repeat_with(|| DeployHashWithApprovals::from(&Deploy::random(rng)))
                    .take(count),
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
            "finalized block #{} in {}, timestamp {}, {} deploys, {} transfers",
            self.height,
            self.era_id,
            self.timestamp,
            self.deploy_hashes.len(),
            self.transfer_hashes.len(),
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
        write!(formatter, "block hash {}", self.0)
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
            "{}, height {} ",
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
        write!(formatter, "era end: {} ", self.era_report)
    }
}

impl DocExample for EraEnd {
    fn doc_example() -> &'static Self {
        &*ERA_END
    }
}

/// The header portion of a [`Block`](struct.Block.html).
#[derive(Clone, DataSize, Eq, PartialEq, Serialize, Deserialize, Debug)]
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
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    block_hash: OnceCell<BlockHash>,
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
    pub fn block_hash(&self) -> BlockHash {
        *self.block_hash.get_or_init(|| {
            let serialized_header = Self::serialize(self)
                .unwrap_or_else(|error| panic!("should serialize block header: {}", error));
            BlockHash::new(Digest::hash(&serialized_header))
        })
    }

    /// Sets the block hash without recomputing it.
    ///
    /// Must only be called with the correct hash.
    pub(crate) fn set_block_hash(&self, block_hash: BlockHash) {
        self.block_hash.get_or_init(|| block_hash);
    }

    /// Returns true if block is Genesis.
    /// Genesis child block is from era 0 and height 0.
    pub(crate) fn is_genesis(&self) -> bool {
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
            block_hash: OnceCell::new(),
        };
        Ok((block_header, remainder))
    }
}

impl Item for BlockHeader {
    type Id = BlockHash;

    fn id(&self) -> Self::Id {
        self.block_hash()
    }
}

impl FetcherItem for BlockHeader {
    type ValidationError = Infallible;
    type ValidationMetadata = EmptyValidationMetadata;
    const TAG: Tag = Tag::BlockHeader;

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, DataSize)]
pub struct BlockHeaderWithMetadata {
    pub block_header: BlockHeader,
    pub block_signatures: BlockSignatures,
}

impl Display for BlockHeaderWithMetadata {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}, and {}", self.block_header, self.block_signatures)
    }
}

impl BlockHeaderWithMetadata {
    pub(crate) fn validate(&self) -> Result<(), BlockHeaderWithMetadataValidationError> {
        validate_block_header_and_signature_hash(&self.block_header, &self.block_signatures)
    }
}

/// The body portion of a block.
#[derive(Clone, DataSize, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct BlockBody {
    proposer: PublicKey,
    deploy_hashes: Vec<DeployHash>,
    transfer_hashes: Vec<DeployHash>,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    hash: OnceCell<Digest>,
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
            hash: OnceCell::new(),
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
    pub(crate) fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes()
            .iter()
            .chain(self.transfer_hashes().iter())
    }

    /// Computes the body hash by hashing the serialized bytes.
    pub fn hash(&self) -> Digest {
        *self.hash.get_or_init(|| {
            let serialized_body = self
                .to_bytes()
                .unwrap_or_else(|error| panic!("should serialize block body: {}", error));
            Digest::hash(&serialized_body)
        })
    }
}

impl Display for BlockBody {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block body proposed by {}, {} deploys, {} transfers",
            self.proposer,
            self.deploy_hashes.len(),
            self.transfer_hashes.len()
        )?;
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
            hash: OnceCell::new(),
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

    /// Verify the signatures contained within.
    pub(crate) fn verify(&self) -> Result<(), crypto::Error> {
        for (public_key, signature) in self.proofs.iter() {
            let signature = FinalitySignature {
                block_hash: self.block_hash,
                era_id: self.era_id,
                signature: *signature,
                public_key: public_key.clone(),
                is_verified: OnceCell::new(),
            };
            signature.is_verified()?;
        }
        Ok(())
    }

    pub(crate) fn get_finality_signature(
        &self,
        public_key: &PublicKey,
    ) -> Option<FinalitySignature> {
        self.proofs.get(public_key).map(|signature| {
            FinalitySignature::new(self.block_hash, self.era_id, *signature, public_key.clone())
        })
    }

    pub(crate) fn finality_signatures(&self) -> impl Iterator<Item = FinalitySignature> + '_ {
        self.proofs.iter().map(move |(public_key, signature)| {
            FinalitySignature::new(self.block_hash, self.era_id, *signature, public_key.clone())
        })
    }

    pub(crate) fn public_keys(&self) -> Option<Vec<PublicKey>> {
        if self.proofs.is_empty() {
            None
        } else {
            let public_keys = self.proofs.keys().cloned().collect_vec();
            Some(public_keys)
        }
    }
}

impl Display for BlockSignatures {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block signatures for {} in {} with {} proofs",
            self.block_hash,
            self.era_id,
            self.proofs.len()
        )
    }
}

/// A proposed block after execution, with the resulting post-state-hash.  This is the core
/// component of the Casper linear blockchain.
#[derive(DataSize, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            block_hash: OnceCell::new(),
        };

        Ok(Block {
            hash: header.block_hash(),
            header,
            body,
        })
    }

    pub(crate) fn new_from_header_and_body(
        header: BlockHeader,
        body: BlockBody,
    ) -> Result<Self, BlockValidationError> {
        let hash = header.block_hash();
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

    /// The list of deploy hashes chained with the list of transfer hashes.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.body.deploy_and_transfer_hashes()
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
        if self.header.is_genesis() {
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
        let actual_block_header_hash = self.header().block_hash();
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
        self.hash = self.header.block_hash();
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

    /// Generates a random instance using a `TestRng`, but using the specified values.
    #[cfg(any(feature = "testing", test))]
    #[allow(clippy::too_many_arguments)]
    pub fn random_with_specifics_and_parent_and_validator_weights<
        'a,
        I: IntoIterator<Item = &'a Deploy>,
    >(
        rng: &mut TestRng,
        era_id: EraId,
        height: u64,
        protocol_version: ProtocolVersion,
        is_switch: bool,
        deploys_iter: I,
        parent_hash: Option<BlockHash>,
        validator_weights: BTreeMap<PublicKey, U512>,
    ) -> Self {
        let parent_hash = match parent_hash {
            Some(parent_hash) => parent_hash,
            None => BlockHash::new(rng.gen::<[u8; Digest::LENGTH]>().into()),
        };
        let state_root_hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        let mut finalized_block =
            FinalizedBlock::random_with_specifics(rng, era_id, height, is_switch, deploys_iter);
        if !validator_weights.is_empty() {
            finalized_block.era_report = Box::new(Some(EraReport::default()));
        }
        let parent_seed = rng.gen::<[u8; Digest::LENGTH]>().into();
        let next_era_validator_weights = if validator_weights.is_empty() {
            None
        } else {
            Some(validator_weights)
        };

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
            "executed block #{}, {}, timestamp {}, {}, parent {}, post-state hash {}, body hash {}, \
             random bit {}, protocol version: {}",
            self.header.height,
            self.hash,
            self.header.timestamp,
            self.header.era_id,
            self.header.parent_hash.inner(),
            self.header.state_root_hash,
            self.header.body_hash,
            self.header.random_bit,
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

    fn id(&self) -> Self::Id {
        *self.hash()
    }
}

impl FetcherItem for Block {
    type ValidationError = BlockValidationError;
    type ValidationMetadata = EmptyValidationMetadata;
    const TAG: Tag = Tag::Block;

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.verify()
    }
}

impl GossiperItem for Block {
    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = true;

    fn target(&self) -> GossipTarget {
        GossipTarget::NonValidators(self.header.era_id)
    }
}

/// A wrapper around `Block` for the purposes of fetching blocks by height in linear chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockWithMetadata {
    pub block: Block,
    pub block_signatures: BlockSignatures,
}

impl Display for BlockWithMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "block #{}, {}, with {} block signatures",
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
    if block_header.block_hash() != finality_signatures.block_hash {
        return Err(
            BlockHeaderWithMetadataValidationError::FinalitySignaturesHaveUnexpectedBlockHash {
                expected_block_hash: block_header.block_hash(),
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

#[derive(DataSize, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Wrapper around block and its deploys.
pub struct BlockAndDeploys {
    /// Block part.
    pub block: Block,
    /// Deploys.
    pub deploys: Vec<Deploy>,
}

impl Display for BlockAndDeploys {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "block {} and deploys", self.block.hash().inner())
    }
}

/// Represents execution results for all deploys in a single block or a chunk of this complete
/// value.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, DataSize)]
pub struct BlockExecutionResultsOrChunk {
    /// Block to which this value or chunk refers to.
    block_hash: BlockHash,
    /// Complete execution results for the block or a chunk of the complete data.
    value: ValueOrChunk<Vec<casper_types::ExecutionResult>>,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    is_valid: OnceCell<Result<bool, bytesrepr::Error>>,
}

impl BlockExecutionResultsOrChunk {
    /// Verifies equivalence of the effects (or chunks) Merkle root hash with the expected value.
    pub fn validate(&self, expected_merkle_root: &Digest) -> Result<bool, bytesrepr::Error> {
        self.is_valid
            .get_or_init(|| match &self.value {
                ValueOrChunk::Value(block_execution_results) => {
                    Ok(&Chunkable::hash(&block_execution_results)? == expected_merkle_root)
                }
                ValueOrChunk::ChunkWithProof(chunk_with_proof) => {
                    Ok(&chunk_with_proof.proof().root_hash() == expected_merkle_root)
                }
            })
            .clone()
    }

    /// Consumes `self` and returns inner `ValueOrChunk` field.
    pub fn into_value(self) -> ValueOrChunk<Vec<casper_types::ExecutionResult>> {
        self.value
    }

    /// Returns the hash of the block this execution result belongs to.
    pub fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }
}

impl Item for BlockExecutionResultsOrChunk {
    type Id = BlockExecutionResultsOrChunkId;

    fn id(&self) -> Self::Id {
        let chunk_index = match &self.value {
            ValueOrChunk::Value(_) => 0,
            ValueOrChunk::ChunkWithProof(chunks) => chunks.proof().index(),
        };
        BlockExecutionResultsOrChunkId {
            chunk_index,
            block_hash: self.block_hash,
        }
    }
}

impl FetcherItem for BlockExecutionResultsOrChunk {
    type ValidationError = ChunkWithProofVerificationError;
    type ValidationMetadata = ExecutionResultsChecksum;
    const TAG: Tag = Tag::BlockExecutionResults;

    fn validate(&self, metadata: &ExecutionResultsChecksum) -> Result<(), Self::ValidationError> {
        if let ValueOrChunk::ChunkWithProof(chunk_with_proof) = &self.value {
            chunk_with_proof.verify()?;
        }
        if let ExecutionResultsChecksum::Checkable(expected) = *metadata {
            if !self
                .validate(&expected)
                .map_err(ChunkWithProofVerificationError::Bytesrepr)?
            {
                return Err(ChunkWithProofVerificationError::UnexpectedRootHash);
            }
        }
        Ok(())
    }
}

impl Display for BlockExecutionResultsOrChunk {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "block execution results (or chunk) for block {}",
            self.block_hash.inner()
        )
    }
}

/// ID of the request for block execution results or chunk.
#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct BlockExecutionResultsOrChunkId {
    /// Index of the chunk being requested.
    chunk_index: u64,
    /// Hash of the block.
    block_hash: BlockHash,
}

impl BlockExecutionResultsOrChunkId {
    /// Returns an instance of post-1.5 request for block execution results.
    /// The `chunk_index` is set to 0 as the starting point of the fetch cycle.
    /// If the effects are stored without chunking the index will be 0 as well.
    pub fn new(block_hash: BlockHash) -> Self {
        BlockExecutionResultsOrChunkId {
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
        BlockExecutionResultsOrChunkId {
            chunk_index: next_chunk,
            block_hash: self.block_hash,
        }
    }

    pub(crate) fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    pub(crate) fn chunk_index(&self) -> u64 {
        self.chunk_index
    }

    /// Constructs a response for the request, retaining the requests' variant and `block_hash`.
    pub(crate) fn response(
        &self,
        value: ValueOrChunk<Vec<casper_types::ExecutionResult>>,
    ) -> BlockExecutionResultsOrChunk {
        BlockExecutionResultsOrChunk {
            block_hash: self.block_hash,
            value,
            is_valid: OnceCell::new(),
        }
    }
}

impl Display for BlockExecutionResultsOrChunkId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "execution results for {} or chunk #{}",
            self.block_hash, self.chunk_index
        )
    }
}

/// Helper struct to on-demand deserialize a trie or chunk ID for display purposes.
pub struct BlockExecutionResultsOrChunkIdDisplay<'a>(pub &'a [u8]);

impl<'a> Display for BlockExecutionResultsOrChunkIdDisplay<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        BlockExecutionResultsOrChunkId::fmt_serialized(f, self.0)
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
                block_hash: OnceCell::new(),
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
                hash: OnceCell::new(),
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
#[derive(Debug, Clone, Serialize, Deserialize, DataSize, Eq, JsonSchema)]
pub struct FinalitySignature {
    /// Hash of a block this signature is for.
    pub block_hash: BlockHash,
    /// Era in which the block was created in.
    pub era_id: EraId,
    /// Signature over the block hash.
    pub signature: Signature,
    /// Public key of the signing validator.
    pub public_key: PublicKey,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    is_verified: OnceCell<Result<(), crypto::Error>>,
}

impl FinalitySignature {
    /// Create an instance of `FinalitySignature`.
    pub fn create(
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
            is_verified: OnceCell::with_value(Ok(())),
        }
    }

    /// Create an instance of `FinalitySignature`.
    pub fn new(
        block_hash: BlockHash,
        era_id: EraId,
        signature: Signature,
        public_key: PublicKey,
    ) -> Self {
        FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
            is_verified: OnceCell::new(),
        }
    }

    /// Verifies whether the signature is correct.
    pub fn is_verified(&self) -> Result<(), crypto::Error> {
        self.is_verified
            .get_or_init(|| {
                // NOTE: This needs to be in sync with the `new` constructor.
                let mut bytes = self.block_hash.inner().into_vec();
                bytes.extend_from_slice(&self.era_id.to_le_bytes());
                crypto::verify(bytes, &self.signature, &self.public_key)
            })
            .clone()
    }

    /// Returns a random `FinalitySignature` for the provided `block_hash` and `era_id`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_for_block(block_hash: BlockHash, era_id: u64) -> Self {
        let (sec_key, pub_key) = generate_ed25519_keypair();
        FinalitySignature::create(block_hash, EraId::new(era_id), &sec_key, pub_key)
    }
}

impl Hash for FinalitySignature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Ensure we initialize self.is_verified field.
        let is_verified = self.is_verified().is_ok();
        // Destructure to make sure we don't accidentally omit fields.
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
            is_verified: _,
        } = self;
        block_hash.hash(state);
        era_id.hash(state);
        signature.hash(state);
        public_key.hash(state);
        is_verified.hash(state);
    }
}

impl PartialEq for FinalitySignature {
    fn eq(&self, other: &FinalitySignature) -> bool {
        // Ensure we initialize self.is_verified field.
        let is_verified = self.is_verified().is_ok();
        // Destructure to make sure we don't accidentally omit fields.
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
            is_verified: _,
        } = self;
        *block_hash == other.block_hash
            && *era_id == other.era_id
            && *signature == other.signature
            && *public_key == other.public_key
            && is_verified == other.is_verified().is_ok()
    }
}

impl Ord for FinalitySignature {
    fn cmp(&self, other: &FinalitySignature) -> Ordering {
        // Ensure we initialize self.is_verified field.
        let is_verified = self.is_verified().is_ok();
        // Destructure to make sure we don't accidentally omit fields.
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
            is_verified: _,
        } = self;
        block_hash
            .cmp(&other.block_hash)
            .then_with(|| era_id.cmp(&other.era_id))
            .then_with(|| signature.cmp(&other.signature))
            .then_with(|| public_key.cmp(&other.public_key))
            .then_with(|| is_verified.cmp(&other.is_verified().is_ok()))
    }
}

impl PartialOrd for FinalitySignature {
    fn partial_cmp(&self, other: &FinalitySignature) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Display for FinalitySignature {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "finality signature for {}, from {}",
            self.block_hash, self.public_key
        )
    }
}

impl Item for FinalitySignature {
    type Id = FinalitySignatureId;

    fn id(&self) -> Self::Id {
        FinalitySignatureId {
            block_hash: self.block_hash,
            era_id: self.era_id,
            public_key: self.public_key.clone(),
        }
    }
}

impl GossiperItem for FinalitySignature {
    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = true;

    fn target(&self) -> GossipTarget {
        GossipTarget::All
    }
}

impl FetcherItem for FinalitySignature {
    type ValidationError = crypto::Error;
    type ValidationMetadata = EmptyValidationMetadata;
    const TAG: Tag = Tag::FinalitySignature;

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.is_verified()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, DataSize)]
pub(crate) struct FinalitySignatureId {
    pub(crate) block_hash: BlockHash,
    pub(crate) era_id: EraId,
    pub(crate) public_key: PublicKey,
}

impl Display for FinalitySignatureId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "finality signature id for {}, from {}",
            self.block_hash, self.public_key
        )
    }
}

/// Returns the hash of the bytesrepr-encoded deploy_ids.
pub(crate) fn compute_approvals_checksum(
    deploy_ids: Vec<DeployId>,
) -> Result<Digest, bytesrepr::Error> {
    let bytes = deploy_ids.into_bytes()?;
    Ok(Digest::hash(bytes))
}

#[cfg(test)]
mod tests {
    use std::{iter, rc::Rc};

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
        random_block.hash = random_block.header.block_hash();
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
                && block.header.block_hash() == actual_block_header_hash => {}
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
        let fs = FinalitySignature::create(*block.hash(), era_id, &secret_rc, public_key.clone());
        assert!(fs.is_verified().is_ok());
        let signature = fs.signature;
        // Verify that signature includes era id.
        let fs_manufactured = FinalitySignature {
            block_hash: *block.hash(),
            era_id: EraId::from(2),
            signature,
            public_key,
            is_verified: OnceCell::new(),
        };
        // Test should fail b/c `signature` is over `era_id=1` and here we're using `era_id=2`.
        assert!(fs_manufactured.is_verified().is_err());
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
}
