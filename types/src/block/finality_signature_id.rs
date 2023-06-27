use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use super::BlockHash;
#[cfg(doc)]
use super::FinalitySignature;
use crate::{EraId, PublicKey};

/// An identifier for a [`FinalitySignature`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct FinalitySignatureId {
    block_hash: BlockHash,
    era_id: EraId,
    public_key: PublicKey,
}

impl FinalitySignatureId {
    /// Returns a new `FinalitySignatureId`.
    pub fn new(block_hash: BlockHash, era_id: EraId, public_key: PublicKey) -> Self {
        FinalitySignatureId {
            block_hash,
            era_id,
            public_key,
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

    /// Returns the public key of the signing validator.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
}

impl Display for FinalitySignatureId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "finality signature id for {}, from {}",
            self.block_hash, self.public_key
        )
    }
}
