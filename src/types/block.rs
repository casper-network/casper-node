use std::fmt::{self, Debug, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    components::storage::BlockType,
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash::{self, Digest},
    },
    utils::DisplayIter,
};

/// A block; the core component of the CasperLabs linear blockchain.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct Block {
    hash: Digest,
    parent_hash: Digest,
    root_state_hash: Digest,
    // consensus_data: ConsensusData,
    era: u64,
    proofs: Vec<Signature>,
}

impl Block {
    /// Constructs a new `Block`.
    // TODO(Fraser): implement properly
    pub fn new(temp: u8) -> Self {
        let hash = hash::hash(&[temp]);
        let parent_hash = hash::hash(&[temp.overflowing_add(1).0]);
        let root_state_hash = hash::hash(&[temp.overflowing_add(2).0]);

        let secret_key = SecretKey::generate_ed25519();
        let public_key = PublicKey::from(&secret_key);

        let proofs = vec![
            asymmetric_key::sign(&[3], &secret_key, &public_key),
            asymmetric_key::sign(&[4], &secret_key, &public_key),
            asymmetric_key::sign(&[5], &secret_key, &public_key),
        ];
        Block {
            hash,
            parent_hash,
            root_state_hash,
            era: temp.into(),
            proofs,
        }
    }
}

impl BlockType for Block {
    type Hash = Digest;

    fn hash(&self) -> &Self::Hash {
        &self.hash
    }
}

impl Display for Block {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "Block {{ hash: {}, parent_hash: {}, root_state_hash: {}, era: {}, proofs: {} }}",
            self.hash,
            self.parent_hash,
            self.root_state_hash,
            self.era,
            DisplayIter::new(self.proofs.iter())
        )
    }
}
