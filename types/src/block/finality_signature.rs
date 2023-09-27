use alloc::vec::Vec;
use core::{
    cmp::Ordering,
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::BlockHash;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{crypto, EraId, PublicKey, SecretKey, Signature};

/// A validator's signature of a block, confirming it is finalized.
///
/// Clients and joining nodes should wait until the signers' combined weight exceeds the fault
/// tolerance threshold before accepting the block as finalized.
#[derive(Clone, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A validator's signature of a block, confirming it is finalized.")
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct FinalitySignature {
    /// The block hash of the associated block.
    pub(super) block_hash: BlockHash,
    /// The era in which the associated block was created.
    pub(super) era_id: EraId,
    /// The signature over the block hash of the associated block.
    pub(super) signature: Signature,
    /// The public key of the signing validator.
    pub(super) public_key: PublicKey,
    #[serde(skip)]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    pub(super) is_verified: OnceCell<Result<(), crypto::Error>>,
}

impl FinalitySignature {
    /// Constructs a new `FinalitySignature`.
    pub fn create(block_hash: BlockHash, era_id: EraId, secret_key: &SecretKey) -> Self {
        let bytes = Self::bytes_to_sign(&block_hash, era_id);
        let public_key = PublicKey::from(secret_key);
        let signature = crypto::sign(bytes, secret_key, &public_key);
        FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::with_value(Ok(())),
        }
    }

    /// Returns the block hash of the associated block.
    pub fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    /// Returns the era in which the associated block was created.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the signature over the block hash of the associated block.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    /// Returns the public key of the signing validator.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Returns `Ok` if the signature is cryptographically valid.
    pub fn is_verified(&self) -> Result<(), crypto::Error> {
        #[cfg(any(feature = "once_cell", test))]
        return self.is_verified.get_or_init(|| self.verify()).clone();

        #[cfg(not(any(feature = "once_cell", test)))]
        self.verify()
    }

    /// Constructs a new `FinalitySignature`.
    #[cfg(any(feature = "testing", test))]
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
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        }
    }

    /// Returns a random `FinalitySignature`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        FinalitySignature::random_for_block(BlockHash::random(rng), EraId::random(rng), rng)
    }

    /// Returns a random `FinalitySignature` for the provided `block_hash` and `era_id`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_for_block(block_hash: BlockHash, era_id: EraId, rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        FinalitySignature::create(block_hash, era_id, &secret_key)
    }

    fn bytes_to_sign(block_hash: &BlockHash, era_id: EraId) -> Vec<u8> {
        let mut bytes = block_hash.inner().into_vec();
        bytes.extend_from_slice(&era_id.to_le_bytes());
        bytes
    }

    fn verify(&self) -> Result<(), crypto::Error> {
        let bytes = Self::bytes_to_sign(&self.block_hash, self.era_id);
        crypto::verify(bytes, &self.signature, &self.public_key)
    }
}

impl Hash for FinalitySignature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Ensure we initialize self.is_verified field.
        let is_verified = self.is_verified().is_ok();
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
            is_verified: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
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
        #[cfg(any(feature = "once_cell", test))]
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
            is_verified: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
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
        #[cfg(any(feature = "once_cell", test))]
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
            is_verified: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let FinalitySignature {
            block_hash,
            era_id,
            signature,
            public_key,
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
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finality signature for {}, from {}",
            self.block_hash, self.public_key
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestBlockBuilder;

    #[test]
    fn finality_signature() {
        let rng = &mut TestRng::new();
        let block = TestBlockBuilder::new().build(rng);
        // Signature should be over both block hash and era id.
        let secret_key = SecretKey::random(rng);
        let public_key = PublicKey::from(&secret_key);
        let era_id = EraId::from(1);
        let finality_signature = FinalitySignature::create(*block.hash(), era_id, &secret_key);
        finality_signature.is_verified().unwrap();
        let signature = finality_signature.signature;
        // Verify that signature includes era id.
        let invalid_finality_signature = FinalitySignature {
            block_hash: *block.hash(),
            era_id: EraId::from(2),
            signature,
            public_key,
            is_verified: OnceCell::new(),
        };
        // Test should fail b/c `signature` is over `era_id=1` and here we're using `era_id=2`.
        assert!(invalid_finality_signature.is_verified().is_err());
    }
}
