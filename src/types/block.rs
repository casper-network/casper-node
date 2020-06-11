use std::fmt::{self, Debug, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    components::storage::Value,
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash::{self, Digest},
    },
    utils::DisplayIter,
};

/// The cryptographic hash of a [`Block`](struct.Block.html).
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
    root_state_hash: Digest,
    // consensus_data: ConsensusData,
    era: u64,
    proofs: Vec<Signature>,
}

impl Display for BlockHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block-header[parent_hash: {}, root_state_hash: {}, era: {}, proofs: {}]",
            self.parent_hash,
            self.root_state_hash,
            self.era,
            DisplayIter::new(self.proofs.iter())
        )
    }
}

/// A block; the core component of the CasperLabs linear blockchain.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct Block {
    hash: BlockHash,
    header: BlockHeader,
}

impl Block {
    /// Constructs a new `Block`.
    // TODO(Fraser): implement properly
    pub fn new(temp: u64) -> Self {
        let hash = BlockHash::new(hash::hash(temp.to_le_bytes()));
        let parent_hash = BlockHash::new(hash::hash(temp.overflowing_add(1).0.to_le_bytes()));
        let root_state_hash = hash::hash(temp.overflowing_add(2).0.to_le_bytes());

        let secret_key = SecretKey::generate_ed25519();
        let public_key = PublicKey::from(&secret_key);

        let proofs = vec![
            asymmetric_key::sign(&[3], &secret_key, &public_key),
            asymmetric_key::sign(&[4], &secret_key, &public_key),
            asymmetric_key::sign(&[5], &secret_key, &public_key),
        ];

        let header = BlockHeader {
            parent_hash,
            root_state_hash,
            era: temp,
            proofs,
        };
        Block { hash, header }
    }

    /// Returns the `BlockHash` identifying this `Block`.
    pub fn id(&self) -> &BlockHash {
        &self.hash
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
}

impl Display for Block {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "block[{} {}]", self.hash, self.header)
    }
}
