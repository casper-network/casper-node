#[cfg(test)]
use std::iter;
use std::{
    array::TryFromSliceError,
    collections::BTreeMap,
    convert::TryFrom,
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
};

use hex::FromHexError;
use hex_fmt::{HexFmt, HexList};
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

use super::{Item, Tag, Timestamp};
use crate::{
    components::{consensus::EraId, storage::Value},
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
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
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

    /// Returns `true` is `self` is a hash of empty `ProtoBlock`.
    pub(crate) fn is_empty(self) -> bool {
        self == ProtoBlock::empty_random_bit_false() || self == ProtoBlock::empty_random_bit_true()
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
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProtoBlock {
    hash: ProtoBlockHash,
    deploys: Vec<DeployHash>,
    random_bit: bool,
}

impl ProtoBlock {
    pub(crate) fn new(deploys: Vec<DeployHash>, random_bit: bool) -> Self {
        let hash = ProtoBlockHash::new(hash::hash(
            &rmp_serde::to_vec(&(&deploys, random_bit)).expect("serialize ProtoBlock"),
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

    /// Returns hash of empty ProtoBlock (no deploys) with a random bit set to false.
    /// Added here so that it's always aligned with how hash is calculated.
    pub(crate) fn empty_random_bit_false() -> ProtoBlockHash {
        *ProtoBlock::new(vec![], false).hash()
    }

    /// Returns hash of empty ProtoBlock (no deploys) with a random bit set to true.
    /// Added here so that it's always aligned with how hash is calculated.
    pub(crate) fn empty_random_bit_true() -> ProtoBlockHash {
        *ProtoBlock::new(vec![], true).hash()
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

/// System transactions like slashing and rewards.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SystemTransaction {
    /// A validator has equivocated and should be slashed.
    Slash(PublicKey),
    /// Block reward information, in trillionths (10^-12) of the total reward for one block.
    /// This includes the delegator reward.
    Rewards(BTreeMap<PublicKey, u64>),
}

impl SystemTransaction {
    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            SystemTransaction::Slash(PublicKey::random(rng))
        } else {
            let count = rng.gen_range(2, 11);
            let rewards = iter::repeat_with(|| {
                let public_key = PublicKey::random(rng);
                let amount = rng.gen();
                (public_key, amount)
            })
            .take(count)
            .collect();
            SystemTransaction::Rewards(rewards)
        }
    }
}

impl Display for SystemTransaction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            SystemTransaction::Slash(public_key) => write!(formatter, "slash {}", public_key),
            SystemTransaction::Rewards(rewards) => {
                let rewards = rewards
                    .iter()
                    .map(|(public_key, amount)| format!("{}: {}", public_key, amount))
                    .collect::<Vec<_>>();
                write!(formatter, "rewards [{}]", DisplayIter::new(rewards.iter()))
            }
        }
    }
}

/// The piece of information that will become the content of a future block after it was finalized
/// and before execution happened yet.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FinalizedBlock {
    proto_block: ProtoBlock,
    timestamp: Timestamp,
    system_transactions: Vec<SystemTransaction>,
    switch_block: bool,
    era_id: EraId,
    height: u64,
    proposer: PublicKey,
}

impl FinalizedBlock {
    pub(crate) fn new(
        proto_block: ProtoBlock,
        timestamp: Timestamp,
        system_transactions: Vec<SystemTransaction>,
        switch_block: bool,
        era_id: EraId,
        height: u64,
        proposer: PublicKey,
    ) -> Self {
        FinalizedBlock {
            proto_block,
            timestamp,
            system_transactions,
            switch_block,
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

    /// Instructions for system transactions like slashing and rewards.
    pub(crate) fn system_transactions(&self) -> &Vec<SystemTransaction> {
        &self.system_transactions
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
}

impl From<Block> for FinalizedBlock {
    fn from(b: Block) -> Self {
        let proto_block =
            ProtoBlock::new(b.header().deploy_hashes().clone(), b.header().random_bit);

        let timestamp = b.header().timestamp();
        let switch_block = b.header().switch_block;
        let era_id = b.header().era_id;
        let height = b.header().height;
        let header = b.take_header();
        let proposer = header.proposer;
        let system_transactions = header.system_transactions;

        FinalizedBlock {
            proto_block,
            timestamp,
            system_transactions,
            switch_block,
            era_id,
            height,
            proposer,
        }
    }
}

impl Display for FinalizedBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finalized {}block {:10} in era {:?}, height {}, deploys {:10}, random bit {}, \
            timestamp {}, system_transactions: [{}]",
            if self.switch_block { "switch " } else { "" },
            HexFmt(self.proto_block.hash().inner()),
            self.era_id,
            self.height,
            HexList(&self.proto_block.deploys),
            self.proto_block.random_bit,
            self.timestamp(),
            DisplayIter::new(self.system_transactions().iter())
        )
    }
}

/// A cryptographic hash identifying a [`Block`](struct.Block.html).
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
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

/// The header portion of a [`Block`](struct.Block.html).
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct BlockHeader {
    parent_hash: BlockHash,
    global_state_hash: Digest,
    body_hash: Digest,
    deploy_hashes: Vec<DeployHash>,
    random_bit: bool,
    switch_block: bool,
    timestamp: Timestamp,
    system_transactions: Vec<SystemTransaction>,
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
    pub fn global_state_hash(&self) -> &Digest {
        &self.global_state_hash
    }

    /// The hash of the block's body.
    pub fn body_hash(&self) -> &Digest {
        &self.body_hash
    }

    /// The list of deploy hashes included in the block.
    pub fn deploy_hashes(&self) -> &Vec<DeployHash> {
        &self.deploy_hashes
    }

    /// Returns `true` if this is the last block of an era.
    pub fn switch_block(&self) -> bool {
        self.switch_block
    }

    /// A random bit needed for initializing a future era.
    pub fn random_bit(&self) -> bool {
        self.random_bit
    }

    /// The timestamp from when the proto block was proposed.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Instructions for system transactions like slashing and rewards.
    pub fn system_transactions(&self) -> &Vec<SystemTransaction> {
        &self.system_transactions
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

    // Serialize the block header.
    fn serialize(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec(self)
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
            random bit {}, switch block {}, timestamp {}, system_transactions [{}]",
            self.parent_hash.inner(),
            self.global_state_hash,
            self.body_hash,
            DisplayIter::new(self.deploy_hashes.iter()),
            self.random_bit,
            self.switch_block,
            self.timestamp,
            DisplayIter::new(self.system_transactions.iter()),
        )
    }
}

/// A proto-block after execution, with the resulting post-state-hash.  This is the core component
/// of the Casper linear blockchain.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Block {
    hash: BlockHash,
    header: BlockHeader,
    body: (), // TODO: implement body of block
    proofs: Vec<Signature>,
}

impl Block {
    pub(crate) fn new(
        parent_hash: BlockHash,
        global_state_hash: Digest,
        finalized_block: FinalizedBlock,
    ) -> Self {
        let body = ();
        let serialized_body = Self::serialize_body(&body)
            .unwrap_or_else(|error| panic!("should serialize block body: {}", error));
        let body_hash = hash::hash(&serialized_body);

        let era_id = finalized_block.era_id();
        let height = finalized_block.height();

        let header = BlockHeader {
            parent_hash,
            global_state_hash,
            body_hash,
            deploy_hashes: finalized_block.proto_block.deploys,
            random_bit: finalized_block.proto_block.random_bit,
            switch_block: finalized_block.switch_block,
            timestamp: finalized_block.timestamp,
            system_transactions: finalized_block.system_transactions,
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

    pub(crate) fn hash(&self) -> &BlockHash {
        &self.hash
    }

    pub(crate) fn parent_hash(&self) -> &BlockHash {
        self.header.parent_hash()
    }

    pub(crate) fn global_state_hash(&self) -> &Digest {
        self.header.global_state_hash()
    }

    pub(crate) fn deploy_hashes(&self) -> &Vec<DeployHash> {
        self.header.deploy_hashes()
    }

    pub(crate) fn height(&self) -> u64 {
        self.header.height()
    }

    pub(crate) fn is_genesis_child(&self) -> bool {
        self.header.era_id == EraId(0) && self.header.height == 0
    }

    /// Appends the given signature to this block's proofs.  It should have been validated prior to
    /// this via `BlockHash::verify()`.
    pub(crate) fn append_proof(&mut self, proof: Signature) {
        self.proofs.push(proof)
    }

    /// Convert the `Block` to a JSON value.
    pub fn to_json(&self) -> JsonValue {
        let json_block = json::JsonBlock::from(self);
        json!(json_block)
    }

    /// Try to convert the JSON value to a `Block`.
    pub fn from_json(input: JsonValue) -> Result<Self, Error> {
        let json: json::JsonBlock = serde_json::from_value(input)?;
        Block::try_from(json)
    }

    fn serialize_body(body: &()) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec(body)
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
        let system_transactions_count = rng.gen_range(1, 11);
        let system_transactions = iter::repeat_with(|| SystemTransaction::random(rng))
            .take(system_transactions_count)
            .collect();
        let switch_block = rng.gen_bool(0.1);
        let era = rng.gen_range(0, 5);
        let secret_key: SecretKey = SecretKey::new_ed25519(rng.gen());
        let public_key = PublicKey::from(&secret_key);

        let finalized_block = FinalizedBlock::new(
            proto_block,
            timestamp,
            system_transactions,
            switch_block,
            EraId(era),
            era * 10 + rng.gen_range(0, 10),
            public_key,
        );

        let parent_hash = BlockHash::new(Digest::random(rng));
        let global_state_hash = Digest::random(rng);
        let mut block = Block::new(parent_hash, global_state_hash, finalized_block);

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
            random bit {}, timestamp {}, era_id {}, height {}, system_transactions [{}], proofs count {}",
            self.hash.inner(),
            self.header.parent_hash.inner(),
            self.header.global_state_hash,
            self.header.body_hash,
            DisplayIter::new(self.header.deploy_hashes.iter()),
            self.header.random_bit,
            self.header.timestamp,
            self.header.era_id.0,
            self.header.height,
            DisplayIter::new(self.header.system_transactions.iter()),
            self.proofs.len()
        )
    }
}

impl BlockLike for Block {
    fn deploys(&self) -> &Vec<DeployHash> {
        self.deploy_hashes()
    }
}

impl Value for Block {
    type Id = BlockHash;
    type Header = BlockHeader;

    fn id(&self) -> &Self::Id {
        &self.hash
    }

    fn header(&self) -> &Self::Header {
        &self.header
    }

    fn take_header(self) -> Self::Header {
        self.header
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

/// This module provides structs which map to the main block types, but which are suitable for
/// encoding to and decoding from JSON.  For all fields with binary data, this is converted to/from
/// hex strings.
mod json {
    use std::convert::{TryFrom, TryInto};

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        crypto::{
            asymmetric_key::{PublicKey, Signature},
            hash::Digest,
        },
        types::Timestamp,
    };

    #[derive(Serialize, Deserialize)]
    struct JsonBlockHash(String);

    impl From<&BlockHash> for JsonBlockHash {
        fn from(hash: &BlockHash) -> Self {
            JsonBlockHash(hex::encode(hash.0))
        }
    }

    impl TryFrom<JsonBlockHash> for BlockHash {
        type Error = Error;

        fn try_from(hash: JsonBlockHash) -> Result<Self, Self::Error> {
            let hash = Digest::from_hex(&hash.0)
                .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;
            Ok(BlockHash(hash))
        }
    }

    #[derive(Serialize, Deserialize)]
    enum JsonSystemTransaction {
        Slash(String),
        Rewards(BTreeMap<String, u64>),
    }

    impl From<&SystemTransaction> for JsonSystemTransaction {
        fn from(txn: &SystemTransaction) -> Self {
            match txn {
                SystemTransaction::Slash(public_key) => {
                    JsonSystemTransaction::Slash(public_key.to_hex())
                }
                SystemTransaction::Rewards(map) => JsonSystemTransaction::Rewards(
                    map.iter()
                        .map(|(public_key, amount)| (public_key.to_hex(), *amount))
                        .collect(),
                ),
            }
        }
    }

    impl TryFrom<JsonSystemTransaction> for SystemTransaction {
        type Error = Error;

        fn try_from(json_txn: JsonSystemTransaction) -> Result<Self, Self::Error> {
            match json_txn {
                JsonSystemTransaction::Slash(hex_public_key) => Ok(SystemTransaction::Slash(
                    PublicKey::from_hex(&hex_public_key)
                        .map_err(|error| Error::DecodeFromJson(Box::new(error)))?,
                )),
                JsonSystemTransaction::Rewards(json_map) => {
                    let mut map = BTreeMap::new();
                    for (hex_public_key, amount) in json_map.iter() {
                        let public_key = PublicKey::from_hex(&hex_public_key)
                            .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;
                        let _ = map.insert(public_key, *amount);
                    }
                    Ok(SystemTransaction::Rewards(map))
                }
            }
        }
    }

    #[derive(Serialize, Deserialize)]
    struct JsonBlockHeader {
        parent_hash: String,
        global_state_hash: String,
        body_hash: String,
        deploy_hashes: Vec<String>,
        random_bit: bool,
        switch_block: bool,
        timestamp: Timestamp,
        system_transactions: Vec<JsonSystemTransaction>,
        era_id: EraId,
        height: u64,
        proposer: String,
    }

    impl From<&BlockHeader> for JsonBlockHeader {
        fn from(header: &BlockHeader) -> Self {
            JsonBlockHeader {
                parent_hash: hex::encode(header.parent_hash),
                global_state_hash: hex::encode(header.global_state_hash),
                body_hash: hex::encode(header.body_hash),
                deploy_hashes: header
                    .deploy_hashes
                    .iter()
                    .map(|deploy_hash| hex::encode(deploy_hash.as_ref()))
                    .collect(),
                random_bit: header.random_bit,
                switch_block: header.switch_block,
                timestamp: header.timestamp,
                system_transactions: header.system_transactions.iter().map(Into::into).collect(),
                era_id: header.era_id,
                height: header.height,
                proposer: header.proposer.to_hex(),
            }
        }
    }

    impl TryFrom<JsonBlockHeader> for BlockHeader {
        type Error = Error;

        fn try_from(header: JsonBlockHeader) -> Result<Self, Self::Error> {
            let mut system_transactions = vec![];
            for json_txn in header.system_transactions {
                let txn = json_txn.try_into()?;
                system_transactions.push(txn);
            }

            let mut deploy_hashes = vec![];
            for hex_deploy_hash in header.deploy_hashes.iter() {
                let hash = Digest::from_hex(&hex_deploy_hash)
                    .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;
                deploy_hashes.push(DeployHash::from(hash));
            }

            let proposer = PublicKey::from_hex(&header.proposer)
                .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;

            Ok(BlockHeader {
                parent_hash: BlockHash::from(
                    Digest::from_hex(&header.parent_hash)
                        .map_err(|error| Error::DecodeFromJson(Box::new(error)))?,
                ),
                global_state_hash: Digest::from_hex(&header.global_state_hash)
                    .map_err(|error| Error::DecodeFromJson(Box::new(error)))?,
                body_hash: Digest::from_hex(&header.body_hash)
                    .map_err(|error| Error::DecodeFromJson(Box::new(error)))?,
                deploy_hashes,
                random_bit: header.random_bit,
                switch_block: header.switch_block,
                timestamp: header.timestamp,
                system_transactions,
                era_id: header.era_id,
                height: header.height,
                proposer,
            })
        }
    }

    #[derive(Serialize, Deserialize)]
    pub(super) struct JsonBlock {
        hash: JsonBlockHash,
        header: JsonBlockHeader,
        proofs: Vec<String>,
    }

    impl From<&Block> for JsonBlock {
        fn from(block: &Block) -> Self {
            JsonBlock {
                hash: (&block.hash).into(),
                header: (&block.header).into(),
                proofs: block.proofs.iter().map(Signature::to_hex).collect(),
            }
        }
    }

    impl TryFrom<JsonBlock> for Block {
        type Error = Error;

        fn try_from(block: JsonBlock) -> Result<Self, Self::Error> {
            let mut proofs = vec![];
            for json_proof in block.proofs.iter() {
                let proof = Signature::from_hex(json_proof)
                    .map_err(|error| Error::DecodeFromJson(Box::new(error)))?;
                proofs.push(proof);
            }
            Ok(Block {
                hash: block.hash.try_into()?,
                header: block.header.try_into()?,
                body: (),
                proofs,
            })
        }
    }
}

/// A wrapper around `Block` for the purposes of fetching blocks by height in linear chain.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockByHeight {
    Absent(u64),
    Block(Box<Block>),
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
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self)
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
    use std::str::FromStr;

    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn json_roundtrip() {
        let mut rng = TestRng::new();
        let block = Block::random(&mut rng);
        let json_string = block.to_json().to_string();
        let json = JsonValue::from_str(json_string.as_str()).unwrap();
        let decoded = Block::from_json(json).unwrap();
        assert_eq!(block, decoded);
    }
}
