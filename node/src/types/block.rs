#[cfg(test)]
use std::iter;
use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Display, Formatter},
};

use hex_fmt::{HexFmt, HexList};
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

use super::Timestamp;
use crate::{
    components::storage::Value,
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
    parent_hash: ProtoBlockHash,
    deploys: Vec<DeployHash>,
    random_bit: bool,
    switch_block: bool,
}

impl ProtoBlock {
    pub(crate) fn new(
        parent_hash: ProtoBlockHash,
        deploys: Vec<DeployHash>,
        random_bit: bool,
        switch_block: bool,
    ) -> Self {
        let hash = ProtoBlockHash::new(hash::hash(
            &rmp_serde::to_vec(&(&deploys, random_bit)).expect("serialize ProtoBlock"),
        ));

        ProtoBlock {
            parent_hash,
            hash,
            deploys,
            random_bit,
            switch_block,
        }
    }

    pub(crate) fn hash(&self) -> &ProtoBlockHash {
        &self.hash
    }

    pub(crate) fn parent_hash(&self) -> &ProtoBlockHash {
        &self.parent_hash
    }

    /// The list of deploy hashes included in the block.
    pub(crate) fn deploys(&self) -> &Vec<DeployHash> {
        &self.deploys
    }

    /// A random bit needed for initializing a future era.
    pub(crate) fn random_bit(&self) -> bool {
        self.random_bit
    }

    /// Returns `true` if this is the last block of an era.
    pub(crate) fn switch_block(&self) -> bool {
        self.switch_block
    }

    pub(crate) fn destructure(self) -> (ProtoBlockHash, ProtoBlockHash, Vec<DeployHash>, bool) {
        (self.hash, self.parent_hash, self.deploys, self.random_bit)
    }
}

impl Display for ProtoBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "proto block {}, parent hash {}, deploys [{}], random bit {}",
            self.hash.inner(),
            self.parent_hash.inner(),
            DisplayIter::new(self.deploys.iter()),
            self.random_bit(),
        )
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
}

impl FinalizedBlock {
    pub(crate) fn new(
        proto_block: ProtoBlock,
        timestamp: Timestamp,
        system_transactions: Vec<SystemTransaction>,
    ) -> Self {
        FinalizedBlock {
            proto_block,
            timestamp,
            system_transactions,
        }
    }

    /// The finalized proto block.
    pub(crate) fn proto_block(&self) -> &ProtoBlock {
        &self.proto_block
    }

    /// The timestamp from when the proto block was proposed.
    pub(crate) fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }

    /// Instructions for system transactions like slashing and rewards.
    pub(crate) fn system_transactions(&self) -> &Vec<SystemTransaction> {
        &self.system_transactions
    }
}

impl Display for FinalizedBlock {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finalized block {:10} deploys {:10}, random bit {}, timestamp {}, \
            system_transactions: [{}]",
            HexFmt(self.proto_block.hash().inner()),
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

/// The header portion of a [`Block`](struct.Block.html).
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct BlockHeader {
    parent_hash: BlockHash,
    post_state_hash: Digest,
    body_hash: Digest,
    deploy_hashes: Vec<DeployHash>,
    random_bit: bool,
    timestamp: Timestamp,
    system_transactions: Vec<SystemTransaction>,
}

impl BlockHeader {
    /// The parent block's hash.
    pub fn parent_hash(&self) -> &BlockHash {
        &self.parent_hash
    }

    /// The root hash of the resulting global state.
    pub fn post_state_hash(&self) -> &Digest {
        &self.post_state_hash
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
    pub fn random_bit(&self) -> &bool {
        &self.random_bit
    }

    /// The timestamp from when the proto block was proposed.
    pub fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }

    /// Instructions for system transactions like slashing and rewards.
    pub fn system_transactions(&self) -> &Vec<SystemTransaction> {
        &self.system_transactions
    }
}

impl Display for BlockHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block header parent hash {}, post-state hash {}, body hash {}, deploys [{}], \
            random bit {}, timestamp {}, system_transactions [{}]",
            self.parent_hash.inner(),
            self.post_state_hash,
            self.body_hash,
            DisplayIter::new(self.deploy_hashes.iter()),
            self.random_bit,
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
        post_state_hash: Digest,
        finalized_block: FinalizedBlock,
    ) -> Self {
        let body = ();
        let serialized_body = Self::serialize_body(&body)
            .unwrap_or_else(|error| panic!("should serialize block body: {}", error));
        let body_hash = hash::hash(&serialized_body);

        let header = BlockHeader {
            parent_hash,
            post_state_hash,
            body_hash,
            deploy_hashes: finalized_block.proto_block.deploys,
            random_bit: finalized_block.proto_block.random_bit,
            timestamp: finalized_block.timestamp,
            system_transactions: finalized_block.system_transactions,
        };
        let serialized_header = Self::serialize_header(&header)
            .unwrap_or_else(|error| panic!("should serialize block header: {}", error));
        let hash = BlockHash::new(hash::hash(&serialized_header));

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

    /// Appends the given signature to this block's proofs.  It should have been validated prior to
    /// this via `BlockHash::verify()`.
    pub(crate) fn append_proof(&mut self, proof: Signature) {
        self.proofs.push(proof)
    }

    fn serialize_header(header: &BlockHeader) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec(header)
    }

    fn serialize_body(body: &()) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec(body)
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let proto_parent_hash = ProtoBlockHash(Digest::random(rng));
        let deploy_count = rng.gen_range(0, 11);
        let deploy_hashes = iter::repeat_with(|| DeployHash::new(Digest::random(rng)))
            .take(deploy_count)
            .collect();
        let random_bit = rng.gen();
        let switch_block = rng.gen();
        let proto_block =
            ProtoBlock::new(proto_parent_hash, deploy_hashes, random_bit, switch_block);

        // TODO - make Timestamp deterministic.
        let timestamp = Timestamp::now();
        let system_transactions_count = rng.gen_range(1, 11);
        let system_transactions = iter::repeat_with(|| SystemTransaction::random(rng))
            .take(system_transactions_count)
            .collect();
        let finalized_block = FinalizedBlock::new(proto_block, timestamp, system_transactions);

        let parent_hash = BlockHash::new(Digest::random(rng));
        let post_state_hash = Digest::random(rng);
        let mut block = Block::new(parent_hash, post_state_hash, finalized_block);

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
            random bit {}, timestamp {}, system_transactions [{}], proofs count {}",
            self.hash.inner(),
            self.header.parent_hash.inner(),
            self.header.post_state_hash,
            self.header.body_hash,
            DisplayIter::new(self.header.deploy_hashes.iter()),
            self.header.random_bit,
            self.header.timestamp,
            DisplayIter::new(self.header.system_transactions.iter()),
            self.proofs.len()
        )
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
