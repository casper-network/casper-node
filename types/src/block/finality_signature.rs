mod finality_signature_v1;
mod finality_signature_v2;

pub use finality_signature_v1::FinalitySignatureV1;
pub use finality_signature_v2::FinalitySignatureV2;

use core::{
    fmt::{self, Display, Formatter},
    hash::Hash,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{crypto, BlockHash, EraId, PublicKey, Signature};
#[cfg(any(feature = "testing", test))]
use crate::{testing::TestRng, ChainNameDigest};

/// A validator's signature of a block, confirming it is finalized.
///
/// Clients and joining nodes should wait until the signers' combined weight exceeds the fault
/// tolerance threshold before accepting the block as finalized.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A validator's signature of a block, confirming it is finalized.")
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum FinalitySignature {
    /// Version 1 of the finality signature.
    V1(FinalitySignatureV1),
    /// Version 2 of the finality signature.
    V2(FinalitySignatureV2),
}

impl FinalitySignature {
    /// Returns the block hash of the associated block.
    pub fn block_hash(&self) -> &BlockHash {
        match self {
            FinalitySignature::V1(fs) => fs.block_hash(),
            FinalitySignature::V2(fs) => fs.block_hash(),
        }
    }

    /// Returns the era in which the associated block was created.
    pub fn era_id(&self) -> EraId {
        match self {
            FinalitySignature::V1(fs) => fs.era_id(),
            FinalitySignature::V2(fs) => fs.era_id(),
        }
    }

    /// Returns the public key of the signing validator.
    pub fn public_key(&self) -> &PublicKey {
        match self {
            FinalitySignature::V1(fs) => fs.public_key(),
            FinalitySignature::V2(fs) => fs.public_key(),
        }
    }

    /// Returns the signature over the block hash of the associated block.
    pub fn signature(&self) -> &Signature {
        match self {
            FinalitySignature::V1(fs) => fs.signature(),
            FinalitySignature::V2(fs) => fs.signature(),
        }
    }

    /// Returns `Ok` if the signature is cryptographically valid.
    pub fn is_verified(&self) -> Result<(), crypto::Error> {
        match self {
            FinalitySignature::V1(fs) => fs.is_verified(),
            FinalitySignature::V2(fs) => fs.is_verified(),
        }
    }

    /// Returns a random `FinalitySignature`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let block_hash = BlockHash::random(rng);
        let block_height = rng.gen();
        let era_id = EraId::random(rng);
        let chain_name_hash = ChainNameDigest::random(rng);
        Self::random_for_block(block_hash, block_height, era_id, chain_name_hash, rng)
    }

    /// Returns a random `FinalitySignature` for the provided `block_hash` and `era_id`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_for_block(
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
        chain_name_hash: ChainNameDigest,
        rng: &mut TestRng,
    ) -> Self {
        if rng.gen_bool(0.5) {
            FinalitySignature::V1(FinalitySignatureV1::random_for_block(
                block_hash, era_id, rng,
            ))
        } else {
            FinalitySignature::V2(FinalitySignatureV2::random_for_block(
                block_hash,
                block_height,
                era_id,
                chain_name_hash,
                rng,
            ))
        }
    }
}

impl From<FinalitySignatureV1> for FinalitySignature {
    fn from(fs: FinalitySignatureV1) -> Self {
        FinalitySignature::V1(fs)
    }
}

impl From<FinalitySignatureV2> for FinalitySignature {
    fn from(fs: FinalitySignatureV2) -> Self {
        FinalitySignature::V2(fs)
    }
}

impl Display for FinalitySignature {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FinalitySignature::V1(fs) => write!(f, "{}", fs),
            FinalitySignature::V2(fs) => write!(f, "{}", fs),
        }
    }
}
