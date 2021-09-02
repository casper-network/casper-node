use std::sync::Arc;

use datasize::DataSize;
use tracing::info;

use casper_types::{PublicKey, SecretKey, Signature};
use hashing::Digest;

use crate::{
    components::consensus::traits::{ConsensusValueT, Context, ValidatorSecret},
    crypto::{self},
    types::BlockPayload,
};

#[derive(DataSize)]
pub struct Keypair {
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

    fn sign(&self, hash: &Digest) -> Signature {
        crypto::sign(hash, self.secret_key.as_ref(), &self.public_key)
    }
}

impl ConsensusValueT for Arc<BlockPayload> {
    fn needs_validation(&self) -> bool {
        !self.transfer_hashes().is_empty() || !self.deploy_hashes().is_empty()
    }
}

/// The collection of types used for cryptography, IDs and blocks in the CasperLabs node.
#[derive(Clone, DataSize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClContext;

impl Context for ClContext {
    type ConsensusValue = Arc<BlockPayload>;
    type ValidatorId = PublicKey;
    type ValidatorSecret = Keypair;
    type Signature = Signature;
    type Hash = Digest;
    type InstanceId = Digest;

    fn hash(data: &[u8]) -> Digest {
        Digest::hash(data)
    }

    fn verify_signature(hash: &Digest, public_key: &PublicKey, signature: &Signature) -> bool {
        if let Err(error) = crypto::verify(hash, signature, public_key) {
            info!(%error, %signature, %public_key, %hash, "failed to validate signature");
            return false;
        }
        true
    }
}
