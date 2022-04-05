// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

#[cfg(test)]
use std::iter;
use std::{
    array::TryFromSliceError,
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;
use derive_more::Into;
use hex_fmt::HexList;
use itertools::Itertools;
use once_cell::sync::Lazy;
#[cfg(test)]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_hashing::Digest;
#[cfg(test)]
use casper_types::system::auction::BLOCK_REWARD;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    EraId, ProtocolVersion, PublicKey, SecretKey, Signature, U512,
};

use crate::{
    components::consensus,
    crypto::{self, AsymmetricKeyExt},
    rpcs::docs::DocExample,
    types::{
        error::{BlockCreationError, BlockValidationError},
        Approval, Deploy, DeployHash, DeployOrTransferHash, DeployWithApprovals, JsonBlock,
        JsonBlockHeader,
    },
    utils::DisplayIter,
};
#[cfg(test)]
use crate::{crypto::generate_ed25519_keypair, testing::TestRng};

use super::{Item, Tag, Timestamp};
use crate::types::error::{
    BlockHeaderWithMetadataValidationError, BlockWithMetadataValidationError,
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
    let verifiable_chunked_hash_activation = EraId::new(17_u64);

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
        verifiable_chunked_hash_activation,
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

// This should be clearly specified because the `verifiable_chunked_hash_activation`
// parameter used in various tests strongly rely on this value.
#[cfg(test)]
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

#[cfg(test)]
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
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let era = rng.gen_range(0..5);
        let height = era * 10 + rng.gen_range(0..10);
        let is_switch = rng.gen_bool(0.1);

        FinalizedBlock::random_with_specifics(rng, EraId::from(era), height, is_switch)
    }

    /// Generates a random instance using a `TestRng`, but using the specified era ID and height.
    #[cfg(test)]
    pub fn random_with_specifics(
        rng: &mut TestRng,
        era_id: EraId,
        height: u64,
        is_switch: bool,
    ) -> Self {
        let deploy_count = rng.gen_range(0..11);
        let deploys = iter::repeat_with(|| {
            DeployWithApprovals::new(
                DeployHash::new(rng.gen::<[u8; Digest::LENGTH]>().into()),
                BTreeSet::new(),
            )
        })
        .take(deploy_count)
        .collect();
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
    #[cfg(test)]
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

    pub(crate) fn hash(&self) -> Digest {
        // Pattern match here leverages compiler to ensure every field is accounted for
        let EraEnd {
            next_era_validator_weights,
            era_report,
        } = self;
        // Sort next era validator weights by descending weight. Assuming the top validators are
        // online, a client can get away with just a few (plus a Merkle proof of the rest)
        // to check finality signatures.
        let descending_validator_weight_hashed_pairs = next_era_validator_weights
            .iter()
            .sorted_by_key(|(_, weight)| Reverse(**weight))
            .map(|(validator_id, weight)| {
                let validator_hash =
                    Digest::hash(validator_id.to_bytes().expect("Could not hash validator"));
                let weight_hash = Digest::hash(weight.to_bytes().expect("Could not hash weight"));
                Digest::hash_pair(&validator_hash, &weight_hash)
            });
        let hashed_next_era_validator_weights =
            Digest::hash_merkle_tree(descending_validator_weight_hashed_pairs);
        let hashed_era_report: Digest = era_report.hash();
        Digest::hash_slice_rfold(&[hashed_next_era_validator_weights, hashed_era_report])
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
    accumulated_seed: Digest,
    era_end: Option<EraEnd>,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
    protocol_version: ProtocolVersion,
}

impl BlockHeader {
    /// The [`HashingAlgorithmVersion`] used for the header (as well as for its corresponding block
    /// body).
    pub fn hashing_algorithm_version(
        &self,
        verifiable_chunked_hash_activation: EraId,
    ) -> HashingAlgorithmVersion {
        if self.era_id < verifiable_chunked_hash_activation {
            HashingAlgorithmVersion::V1
        } else {
            HashingAlgorithmVersion::V2
        }
    }

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
    pub fn hash(&self, verifiable_chunked_hash_activation: EraId) -> BlockHash {
        if self.era_id() < verifiable_chunked_hash_activation {
            self.hash_v1()
        } else {
            self.hash_v2()
        }
    }

    fn hash_v1(&self) -> BlockHash {
        let serialized_header = Self::serialize(self)
            .unwrap_or_else(|error| panic!("should serialize block header: {}", error));
        BlockHash::new(Digest::hash(&serialized_header))
    }

    fn hash_v2(&self) -> BlockHash {
        // Pattern match here leverages compiler to ensure every field is accounted for
        let BlockHeader {
            parent_hash,
            era_id,
            body_hash,
            state_root_hash,
            era_end,
            height,
            timestamp,
            protocol_version,
            random_bit,
            accumulated_seed,
        } = self;

        let hashed_era_end = match era_end {
            None => Digest::SENTINEL_NONE,
            Some(era_end) => era_end.hash(),
        };

        let hashed_era_id = Digest::hash(era_id.to_bytes().expect("Could not serialize era_id"));
        let hashed_height = Digest::hash(height.to_bytes().expect("Could not serialize height"));
        let hashed_timestamp =
            Digest::hash(timestamp.to_bytes().expect("Could not serialize timestamp"));
        let hashed_protocol_version = Digest::hash(
            protocol_version
                .to_bytes()
                .expect("Could not serialize protocol version"),
        );
        let hashed_random_bit = Digest::hash(
            random_bit
                .to_bytes()
                .expect("Could not serialize protocol version"),
        );

        Digest::hash_slice_rfold(&[
            hashed_protocol_version,
            parent_hash.0,
            hashed_era_end,
            *body_hash,
            hashed_era_id,
            *state_root_hash,
            hashed_height,
            hashed_timestamp,
            hashed_random_bit,
            *accumulated_seed,
        ])
        .into()
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

    fn validate(
        &self,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError> {
        validate_block_header_and_signature_hash(
            &self.block_header,
            &self.block_signatures,
            verifiable_chunked_hash_activation,
        )
    }

    fn id(&self, _verifiable_chunked_hash_activation: EraId) -> Self::Id {
        self.block_header.height()
    }
}

/// A node in a Merkle linked-list used for hashing data structures.
#[derive(Debug, Clone)]
pub struct MerkleLinkedListNode<T> {
    value: T,
    merkle_proof_of_rest: Digest,
}

impl<T> MerkleLinkedListNode<T> {
    /// Constructs a new [`MerkleLinkedListNode`].
    pub fn new(value: T, merkle_proof_of_rest: Digest) -> MerkleLinkedListNode<T> {
        MerkleLinkedListNode {
            value,
            merkle_proof_of_rest,
        }
    }

    /// Gets the hash of the rest of the linked list.
    pub fn merkle_proof_of_rest(&self) -> &Digest {
        &self.merkle_proof_of_rest
    }

    /// Converts a [`MerkleLinkedListNode`] into its underlying value.
    pub fn take_value(self) -> T {
        self.value
    }
}

/// A fragment of a Merkle-treeified [`BlockBody`].
///
/// Has the following hash structure:
///
/// ```text
/// merkle_linked_list_node_hash
///     |          \
/// value_hash      merkle_linked_list_node::merkle_proof_of_rest
///     |
/// merkle_linked_list_node::value
/// ```
#[derive(Debug, Clone)]
pub struct MerkleBlockBodyPart<'a, T> {
    value_hash: Digest,
    merkle_linked_list_node: MerkleLinkedListNode<&'a T>,
    merkle_linked_list_node_hash: Digest,
}

impl<'a, T> MerkleBlockBodyPart<'a, T> {
    fn new(value: &T, value_hash: Digest, merkle_proof_of_rest: Digest) -> MerkleBlockBodyPart<T> {
        MerkleBlockBodyPart {
            value_hash,
            merkle_linked_list_node: MerkleLinkedListNode {
                value,
                merkle_proof_of_rest,
            },
            merkle_linked_list_node_hash: Digest::hash_pair(&value_hash, &merkle_proof_of_rest),
        }
    }

    /// The value of the block body part
    pub fn value(&self) -> &'a T {
        self.merkle_linked_list_node.value
    }

    /// The hash of the value and the rest of the linked list as a slice
    pub fn value_and_rest_hashes_pair(&self) -> (Digest, Digest) {
        (
            self.value_hash,
            self.merkle_linked_list_node.merkle_proof_of_rest,
        )
    }

    /// The hash of the value of the block body part
    pub fn value_hash(&self) -> &Digest {
        &self.value_hash
    }

    /// The hash of the linked list node
    pub fn merkle_linked_list_node_hash(&self) -> &Digest {
        &self.merkle_linked_list_node_hash
    }
}

/// A [`BlockBody`] that has been hashed so its parts may be stored as a Merkle linked-list.
///
/// ```text
///                   body_hash (root)
///                   /      \__________________
/// hash(deploy_hashes)      /                  \_____________
///                   hash(transfer_hashes)     /             \
///                                         hash(proposer)   SENTINEL
/// ```
#[derive(Debug, Clone)]
pub struct MerkleBlockBody<'a> {
    /// Merklized `BlockBody::deploy_hashes`.
    pub deploy_hashes: MerkleBlockBodyPart<'a, Vec<DeployHash>>,
    /// Merklized `BlockBody::transfer_hashes`.
    pub transfer_hashes: MerkleBlockBodyPart<'a, Vec<DeployHash>>,
    /// Merklized [`BlockBody::proposer`].
    pub proposer: MerkleBlockBodyPart<'a, PublicKey>,
}

#[cfg(test)]
impl<'a> MerkleBlockBody<'a> {
    /// Takes the hashes and Merkle proofs for a [`MerkleBlockBody`].
    /// The `Digest` triplets contain the node hash, and contained within that node: the value hash
    /// and the Merkle proof of the rest of the tree.
    pub(crate) fn take_hashes_and_proofs(
        self,
    ) -> [(Digest, Digest, Digest); BlockBody::PARTS_COUNT] {
        let MerkleBlockBody {
            deploy_hashes,
            transfer_hashes,
            proposer,
        } = self;
        [
            (
                deploy_hashes.merkle_linked_list_node_hash,
                deploy_hashes.value_hash,
                deploy_hashes.merkle_linked_list_node.merkle_proof_of_rest,
            ),
            (
                transfer_hashes.merkle_linked_list_node_hash,
                transfer_hashes.value_hash,
                transfer_hashes.merkle_linked_list_node.merkle_proof_of_rest,
            ),
            (
                proposer.merkle_linked_list_node_hash,
                proposer.value_hash,
                proposer.merkle_linked_list_node.merkle_proof_of_rest,
            ),
        ]
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

    /// Computes the body hash by hashing the serialized bytes.
    fn hash_v1(&self) -> Digest {
        let serialized_body = self
            .to_bytes()
            .unwrap_or_else(|error| panic!("should serialize block body: {}", error));
        Digest::hash(&serialized_body)
    }

    /// Constructs the block body hashes for the block body.
    pub fn merklize(&self) -> MerkleBlockBody {
        // Pattern match here leverages compiler to ensure every field is accounted for
        let BlockBody {
            deploy_hashes,
            transfer_hashes,
            proposer,
        } = self;

        let proposer = MerkleBlockBodyPart::new(
            proposer,
            Digest::hash(&proposer.to_bytes().expect("Could not serialize proposer")),
            Digest::SENTINEL_RFOLD,
        );

        let transfer_hashes = MerkleBlockBodyPart::new(
            transfer_hashes,
            Digest::hash_merkle_tree(transfer_hashes.iter().cloned().map(Digest::from)),
            proposer.merkle_linked_list_node_hash,
        );

        let deploy_hashes = MerkleBlockBodyPart::new(
            deploy_hashes,
            Digest::hash_merkle_tree(deploy_hashes.iter().cloned().map(Digest::from)),
            transfer_hashes.merkle_linked_list_node_hash,
        );

        MerkleBlockBody {
            deploy_hashes,
            transfer_hashes,
            proposer,
        }
    }

    /// Computes the block body hash.
    fn hash_v2(&self) -> Digest {
        self.merklize().deploy_hashes.merkle_linked_list_node_hash
    }

    /// Computes the block body hash, using the indicated hashing algorithm version.
    pub fn hash(&self, version: HashingAlgorithmVersion) -> Digest {
        match version {
            HashingAlgorithmVersion::V1 => self.hash_v1(),
            HashingAlgorithmVersion::V2 => self.hash_v2(),
        }
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
    pub(crate) fn verify(&self) -> crypto::Result<()> {
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

/// A proto-block after execution, with the resulting post-state-hash.  This is the core component
/// of the Casper linear blockchain.
#[derive(DataSize, Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Block {
    hash: BlockHash,
    header: BlockHeader,
    body: BlockBody,
}

/// The hashing algorithm used for the header and the body of a block
#[derive(PartialEq, Eq, Clone, Copy, Debug, DataSize, Hash)]
pub enum HashingAlgorithmVersion {
    /// Version 1
    V1,
    /// Version 2
    V2,
}

impl Block {
    fn hash_block_body(
        block_era_id: EraId,
        block_body: &BlockBody,
        verifiable_chunked_hash_activation: EraId,
    ) -> Digest {
        if block_era_id < verifiable_chunked_hash_activation {
            block_body.hash_v1()
        } else {
            block_body.hash_v2()
        }
    }

    pub(crate) fn new(
        parent_hash: BlockHash,
        parent_seed: Digest,
        state_root_hash: Digest,
        finalized_block: FinalizedBlock,
        next_era_validator_weights: Option<BTreeMap<PublicKey, U512>>,
        protocol_version: ProtocolVersion,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<Self, BlockCreationError> {
        let body = BlockBody::new(
            *finalized_block.proposer,
            finalized_block.deploy_hashes,
            finalized_block.transfer_hashes,
        );

        let body_hash = Self::hash_block_body(
            finalized_block.era_id,
            &body,
            verifiable_chunked_hash_activation,
        );

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
            hash: header.hash(verifiable_chunked_hash_activation),
            header,
            body,
        })
    }

    pub(crate) fn new_from_header_and_body(
        header: BlockHeader,
        body: BlockBody,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<Self, BlockValidationError> {
        let hash = header.hash(verifiable_chunked_hash_activation);
        let block = Block { hash, header, body };
        block.verify(verifiable_chunked_hash_activation)?;
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
    pub fn verify(
        &self,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), BlockValidationError> {
        let actual_block_header_hash = self.header().hash(verifiable_chunked_hash_activation);
        if *self.hash() != actual_block_header_hash {
            return Err(BlockValidationError::UnexpectedBlockHash {
                block: Box::new(self.to_owned()),
                actual_block_header_hash,
            });
        }

        let actual_block_body_hash = Self::hash_block_body(
            self.header().era_id,
            &self.body,
            verifiable_chunked_hash_activation,
        );
        if self.header.body_hash != actual_block_body_hash {
            return Err(BlockValidationError::UnexpectedBodyHash {
                block: Box::new(self.to_owned()),
                actual_block_body_hash,
            });
        }

        Ok(())
    }

    /// Overrides the height of a block.
    #[cfg(test)]
    pub fn set_height(
        &mut self,
        height: u64,
        verifiable_chunked_hash_activation: EraId,
    ) -> &mut Self {
        self.header.height = height;
        self.hash = self.header.hash(verifiable_chunked_hash_activation);
        self
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let era = rng.gen_range(0..MAX_ERA_FOR_RANDOM_BLOCK);
        let height = era * 10 + rng.gen_range(0..10);
        let is_switch = rng.gen_bool(0.1);
        let verifiable_chunked_hash_activation = EraId::from(rng.gen::<u64>());

        Block::random_with_specifics(
            rng,
            EraId::from(era),
            height,
            ProtocolVersion::V1_0_0,
            is_switch,
            verifiable_chunked_hash_activation,
        )
    }

    /// Generates a random instance using a `TestRng` with the specified
    /// `verifiable_chunked_hash_activation`
    #[cfg(test)]
    pub fn random_with_verifiable_chunked_hash_activation(
        rng: &mut TestRng,
        verifiable_chunked_hash_activation: EraId,
    ) -> Self {
        let era = rng.gen_range(0..MAX_ERA_FOR_RANDOM_BLOCK);
        let height = era * 10 + rng.gen_range(0..10);
        let is_switch = rng.gen_bool(0.1);

        Block::random_with_specifics(
            rng,
            EraId::from(era),
            height,
            ProtocolVersion::V1_0_0,
            is_switch,
            verifiable_chunked_hash_activation,
        )
    }

    /// Generates random instance that is guaranteed to be using
    /// the legacy hashing scheme. Apart from the Block itself
    /// it also returns the EraId used as verifiable_chunked_hash_activation.
    #[cfg(test)]
    pub fn random_v1(rng: &mut TestRng) -> (Self, EraId) {
        let verifiable_chunked_hash_activation = EraId::from(MAX_ERA_FOR_RANDOM_BLOCK + 1);

        (
            Self::random_with_verifiable_chunked_hash_activation(
                rng,
                verifiable_chunked_hash_activation,
            ),
            verifiable_chunked_hash_activation,
        )
    }

    /// Generates random instance that is guaranteed to be using
    /// the merkle tree hashing scheme. Apart from the Block itself
    /// it also returns the EraId used as verifiable_chunked_hash_activation.
    #[cfg(test)]
    pub fn random_v2(rng: &mut TestRng) -> (Self, EraId) {
        let verifiable_chunked_hash_activation = EraId::from(0);

        (
            Self::random_with_verifiable_chunked_hash_activation(
                rng,
                verifiable_chunked_hash_activation,
            ),
            verifiable_chunked_hash_activation,
        )
    }

    /// Generates a random instance using a `TestRng`, but using the specified values.
    #[cfg(test)]
    pub fn random_with_specifics(
        rng: &mut TestRng,
        era_id: EraId,
        height: u64,
        protocol_version: ProtocolVersion,
        is_switch: bool,
        verifiable_chunked_hash_activation: EraId,
    ) -> Self {
        let parent_hash = BlockHash::new(rng.gen::<[u8; Digest::LENGTH]>().into());
        let state_root_hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        let finalized_block = FinalizedBlock::random_with_specifics(rng, era_id, height, is_switch);
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
            verifiable_chunked_hash_activation,
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

    fn validate(
        &self,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError> {
        self.verify(verifiable_chunked_hash_activation)
    }

    fn id(&self, _verifiable_chunked_hash_activation: EraId) -> Self::Id {
        *self.hash()
    }
}

/// A wrapper around `Block` for the purposes of fetching blocks by height in linear chain.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockWithMetadata {
    pub block: Block,
    pub finality_signatures: BlockSignatures,
}

impl Display for BlockWithMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Block at {} with hash {} with {} finality signatures.",
            self.block.height(),
            self.block.hash(),
            self.finality_signatures.proofs.len()
        )
    }
}

fn validate_block_header_and_signature_hash(
    block_header: &BlockHeader,
    finality_signatures: &BlockSignatures,
    verifiable_chunked_hash_activation: EraId,
) -> Result<(), BlockHeaderWithMetadataValidationError> {
    if block_header.hash(verifiable_chunked_hash_activation) != finality_signatures.block_hash {
        return Err(
            BlockHeaderWithMetadataValidationError::FinalitySignaturesHaveUnexpectedBlockHash {
                expected_block_hash: block_header.hash(verifiable_chunked_hash_activation),
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

    fn validate(
        &self,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError> {
        self.block.verify(verifiable_chunked_hash_activation)?;
        validate_block_header_and_signature_hash(
            self.block.header(),
            &self.finality_signatures,
            verifiable_chunked_hash_activation,
        )?;
        Ok(())
    }

    fn id(&self, _verifiable_chunked_hash_activation: EraId) -> Self::Id {
        self.block.height()
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
    pub fn verify(&self) -> crypto::Result<()> {
        // NOTE: This needs to be in sync with the `new` constructor.
        let mut bytes = self.block_hash.inner().into_vec();
        bytes.extend_from_slice(&self.era_id.to_le_bytes());
        crypto::verify(bytes, &self.signature, &self.public_key)
    }

    #[cfg(test)]
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

    use casper_types::bytesrepr;

    use crate::testing::TestRng;

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
        let mut rng = TestRng::from_seed([1u8; 16]);
        let loop_iterations = 50;
        for _ in 0..loop_iterations {
            let (random_v1_block, verifiable_chunked_hash_activation) = Block::random_v1(&mut rng);
            random_v1_block
                .verify(verifiable_chunked_hash_activation)
                .expect("v1 (legacy) block hash should check");
            let (random_v2_block, verifiable_chunked_hash_activation) = Block::random_v2(&mut rng);
            random_v2_block
                .verify(verifiable_chunked_hash_activation)
                .expect("v2 (merkle based) block hash should check");
        }
    }

    #[test]
    fn block_check_bad_body_hash_sad_path() {
        let mut rng = TestRng::from_seed([2u8; 16]);

        let blocks = vec![Block::random_v1(&mut rng), Block::random_v2(&mut rng)];

        blocks
            .into_iter()
            .for_each(|(mut random_block, verifiable_chunked_hash_activation)| {
                let bogus_block_body_hash = Digest::hash(&[0xde, 0xad, 0xbe, 0xef]);
                random_block.header.body_hash = bogus_block_body_hash;
                random_block.hash = random_block.header.hash(verifiable_chunked_hash_activation);
                let bogus_block_hash = random_block.hash;

                match random_block.verify(verifiable_chunked_hash_activation) {
                    Err(BlockValidationError::UnexpectedBodyHash {
                        block,
                        actual_block_body_hash,
                    }) if block.hash == bogus_block_hash
                        && block.header.body_hash == bogus_block_body_hash
                        && block.body.hash(
                            block
                                .header
                                .hashing_algorithm_version(verifiable_chunked_hash_activation),
                        ) == actual_block_body_hash => {}
                    unexpected => panic!("Bad check response: {:?}", unexpected),
                }
            });
    }

    #[test]
    fn block_check_bad_block_hash_sad_path() {
        let mut rng = TestRng::from_seed([3u8; 16]);

        let blocks = vec![Block::random_v1(&mut rng), Block::random_v2(&mut rng)];

        blocks
            .into_iter()
            .for_each(|(mut random_block, verifiable_chunked_hash_activation)| {
                let bogus_block_hash: BlockHash = Digest::hash(&[0xde, 0xad, 0xbe, 0xef]).into();
                random_block.hash = bogus_block_hash;

                // No Eq trait for BlockValidationError, so pattern match
                match random_block.verify(verifiable_chunked_hash_activation) {
                    Err(BlockValidationError::UnexpectedBlockHash {
                        block,
                        actual_block_header_hash,
                    }) if block.hash == bogus_block_hash
                        && block.header.hash(verifiable_chunked_hash_activation)
                            == actual_block_header_hash => {}
                    unexpected => panic!("Bad check response: {:?}", unexpected),
                }
            });
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
    fn block_body_merkle_proof_should_be_correct() {
        let mut rng = TestRng::new();
        let era_id = rng.gen_range(0..10).into();
        let height = rng.gen_range(0..100);
        let protocol_version = ProtocolVersion::from_parts(1, 5, 0);

        // We set the merkle tree hash activation to the very beginning of the
        // chain to make sure all blocks are using the Merkle hashing schema
        let verifiable_chunked_hash_activation = EraId::from(0);

        let is_switch = rng.gen();
        let block = Block::random_with_specifics(
            &mut rng,
            era_id,
            height,
            protocol_version,
            is_switch,
            verifiable_chunked_hash_activation,
        );

        let merkle_block_body = block.body().merklize();

        let hashes = [
            *merkle_block_body.deploy_hashes.value_hash(),
            *merkle_block_body.transfer_hashes.value_hash(),
            *merkle_block_body.proposer.value_hash(),
        ];

        assert_eq!(
            block.header().body_hash(),
            merkle_block_body
                .deploy_hashes
                .merkle_linked_list_node_hash()
        );

        assert_eq!(
            *block.header().body_hash(),
            Digest::hash_slice_rfold(&hashes[..])
        );
    }
}
