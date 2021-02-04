use std::sync::Arc;

use datasize::DataSize;
use tracing::info;

use casper_types::{PublicKey, SecretKey, Signature};

use crate::{
    components::consensus::{
        candidate_block::CandidateBlock,
        traits::{Context, ValidatorSecret},
    },
    crypto::{
        self,
        hash::{self, Digest},
    },
    NodeRng,
};

#[derive(DataSize)]
pub(crate) struct Keypair {
    secret_key: Arc<SecretKey>,
    public_key: PublicKey,
}

impl Keypair {
    pub(crate) fn new(secret_key: Arc<SecretKey>, public_key: PublicKey) -> Self {
        Self {
            secret_key,
            public_key,
        }
    }
}

impl From<Arc<SecretKey>> for Keypair {
    fn from(secret_key: Arc<SecretKey>) -> Self {
        let public_key: PublicKey = secret_key.as_ref().into();
        Self::new(secret_key, public_key)
    }
}

impl ValidatorSecret for Keypair {
    type Hash = Digest;
    type Signature = Signature;

    fn sign(&self, hash: &Digest, rng: &mut NodeRng) -> Signature {
        crypto::sign(hash, self.secret_key.as_ref(), &self.public_key, rng)
    }
}

/// The collection of types used for cryptography, IDs and blocks in the CasperLabs node.
#[derive(Clone, DataSize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ClContext;

impl Context for ClContext {
    type ConsensusValue = CandidateBlock;
    type ValidatorId = PublicKey;
    type ValidatorSecret = Keypair;
    type Signature = Signature;
    type Hash = Digest;
    type InstanceId = Digest;

    fn hash(data: &[u8]) -> Digest {
        hash::hash(data)
    }

    fn verify_signature(hash: &Digest, public_key: &PublicKey, signature: &Signature) -> bool {
        if let Err(error) = crypto::verify(hash, signature, public_key) {
            info!(%error, %signature, %public_key, %hash, "failed to validate signature");
            return false;
        }
        true
    }
}
