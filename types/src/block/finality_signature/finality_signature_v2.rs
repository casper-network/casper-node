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
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{crypto, BlockHash, ChainNameDigest, EraId, PublicKey, SecretKey, Signature};

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
pub struct FinalitySignatureV2 {
    /// The block hash of the associated block.
    pub(crate) block_hash: BlockHash,
    /// The height of the associated block.
    pub(crate) block_height: u64,
    /// The era in which the associated block was created.
    pub(crate) era_id: EraId,
    /// The hash of the chain name of the associated block.
    pub(crate) chain_name_hash: ChainNameDigest,
    /// The signature over the block hash of the associated block.
    pub(crate) signature: Signature,
    /// The public key of the signing validator.
    pub(crate) public_key: PublicKey,
    #[serde(skip)]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    pub(crate) is_verified: OnceCell<Result<(), crypto::Error>>,
}

impl FinalitySignatureV2 {
    /// Constructs a new `FinalitySignatureV2`.
    pub fn create(
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
        chain_name_hash: ChainNameDigest,
        secret_key: &SecretKey,
    ) -> Self {
        let bytes = Self::bytes_to_sign(block_hash, block_height, era_id, chain_name_hash);
        let public_key = PublicKey::from(secret_key);
        let signature = crypto::sign(bytes, secret_key, &public_key);
        FinalitySignatureV2 {
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
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

    /// Returns the height of the associated block.
    pub fn block_height(&self) -> u64 {
        self.block_height
    }

    /// Returns the era in which the associated block was created.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the hash of the chain name of the associated block.
    pub fn chain_name_hash(&self) -> ChainNameDigest {
        self.chain_name_hash
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

    /// Constructs a new `FinalitySignatureV2`.
    #[cfg(any(feature = "testing", test))]
    pub fn new(
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
        chain_name_hash: ChainNameDigest,
        signature: Signature,
        public_key: PublicKey,
    ) -> Self {
        FinalitySignatureV2 {
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            signature,
            public_key,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        }
    }

    /// Returns a random `FinalitySignatureV2`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        FinalitySignatureV2::random_for_block(
            BlockHash::random(rng),
            rng.gen(),
            EraId::random(rng),
            ChainNameDigest::random(rng),
            rng,
        )
    }

    /// Returns a random `FinalitySignatureV2` for the provided `block_hash`, `block_height`,
    /// `era_id`, and `chain_name_hash`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_for_block(
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
        chain_name_hash: ChainNameDigest,
        rng: &mut TestRng,
    ) -> Self {
        let secret_key = SecretKey::random(rng);
        FinalitySignatureV2::create(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            &secret_key,
        )
    }

    fn bytes_to_sign(
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
        chain_name_hash: ChainNameDigest,
    ) -> Vec<u8> {
        let mut bytes = block_hash.inner().into_vec();
        bytes.extend_from_slice(&block_height.to_le_bytes());
        bytes.extend_from_slice(&era_id.to_le_bytes());
        bytes.extend_from_slice(chain_name_hash.inner().as_ref());
        bytes
    }

    fn verify(&self) -> Result<(), crypto::Error> {
        let bytes = Self::bytes_to_sign(
            self.block_hash,
            self.block_height,
            self.era_id,
            self.chain_name_hash,
        );
        crypto::verify(bytes, &self.signature, &self.public_key)
    }
}

impl Hash for FinalitySignatureV2 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Ensure we initialize self.is_verified field.
        let is_verified = self.is_verified().is_ok();
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let FinalitySignatureV2 {
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            signature,
            public_key,
            is_verified: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let FinalitySignatureV2 {
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            signature,
            public_key,
        } = self;
        block_hash.hash(state);
        block_height.hash(state);
        era_id.hash(state);
        chain_name_hash.hash(state);
        signature.hash(state);
        public_key.hash(state);
        is_verified.hash(state);
    }
}

impl PartialEq for FinalitySignatureV2 {
    fn eq(&self, other: &FinalitySignatureV2) -> bool {
        // Ensure we initialize self.is_verified field.
        let is_verified = self.is_verified().is_ok();
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let FinalitySignatureV2 {
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            signature,
            public_key,
            is_verified: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let FinalitySignatureV2 {
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            signature,
            public_key,
        } = self;
        *block_hash == other.block_hash
            && *block_height == other.block_height
            && *era_id == other.era_id
            && *chain_name_hash == other.chain_name_hash
            && *signature == other.signature
            && *public_key == other.public_key
            && is_verified == other.is_verified().is_ok()
    }
}

impl Ord for FinalitySignatureV2 {
    fn cmp(&self, other: &FinalitySignatureV2) -> Ordering {
        // Ensure we initialize self.is_verified field.
        let is_verified = self.is_verified().is_ok();
        // Destructure to make sure we don't accidentally omit fields.
        #[cfg(any(feature = "once_cell", test))]
        let FinalitySignatureV2 {
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            signature,
            public_key,
            is_verified: _,
        } = self;
        #[cfg(not(any(feature = "once_cell", test)))]
        let FinalitySignatureV2 {
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            signature,
            public_key,
        } = self;
        block_hash
            .cmp(&other.block_hash)
            .then_with(|| block_height.cmp(&other.block_height))
            .then_with(|| era_id.cmp(&other.era_id))
            .then_with(|| chain_name_hash.cmp(&other.chain_name_hash))
            .then_with(|| signature.cmp(&other.signature))
            .then_with(|| public_key.cmp(&other.public_key))
            .then_with(|| is_verified.cmp(&other.is_verified().is_ok()))
    }
}

impl PartialOrd for FinalitySignatureV2 {
    fn partial_cmp(&self, other: &FinalitySignatureV2) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Display for FinalitySignatureV2 {
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
        // Signature should be over block hash, block height, era id and chain name hash.
        let secret_key = SecretKey::random(rng);
        let era_id = EraId::from(1);
        let chain_name_hash = ChainNameDigest::from_chain_name("example");
        let finality_signature = FinalitySignatureV2::create(
            *block.hash(),
            block.height(),
            era_id,
            chain_name_hash,
            &secret_key,
        );
        finality_signature.is_verified().unwrap();
        // Verify that changing era causes verification to fail.
        let invalid_finality_signature = FinalitySignatureV2 {
            era_id: EraId::from(2),
            is_verified: OnceCell::new(),
            ..finality_signature.clone()
        };
        assert!(invalid_finality_signature.is_verified().is_err());
        // Verify that changing block height causes verification to fail.
        let invalid_finality_signature = FinalitySignatureV2 {
            block_height: block.height() + 1,
            is_verified: OnceCell::new(),
            ..finality_signature.clone()
        };
        assert!(invalid_finality_signature.is_verified().is_err());
        // Verify that changing chain name hash causes verification to fail.
        let invalid_finality_signature = FinalitySignatureV2 {
            chain_name_hash: ChainNameDigest::from_chain_name("different"),
            is_verified: OnceCell::new(),
            ..finality_signature
        };
        assert!(invalid_finality_signature.is_verified().is_err());
    }
}
