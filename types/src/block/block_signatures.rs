use alloc::collections::BTreeMap;
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use super::{BlockHash, FinalitySignature};
use crate::{crypto, EraId, PublicKey, Signature};

/// An error returned during an attempt to merge two incompatible [`BlockSignatures`].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[non_exhaustive]
pub enum BlockSignaturesMergeError {
    /// A mismatch between block hashes.
    BlockHashMismatch {
        /// The `self` hash.
        self_hash: BlockHash,
        /// The `other` hash.
        other_hash: BlockHash,
    },
    /// A mismatch between era IDs.
    EraIdMismatch {
        /// The `self` era ID.
        self_era_id: EraId,
        /// The `other` era ID.
        other_era_id: EraId,
    },
}

impl Display for BlockSignaturesMergeError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            BlockSignaturesMergeError::BlockHashMismatch {
                self_hash,
                other_hash,
            } => {
                write!(
                    formatter,
                    "mismatch between block hashes while merging block signatures - self: {}, \
                    other: {}",
                    self_hash, other_hash
                )
            }
            BlockSignaturesMergeError::EraIdMismatch {
                self_era_id,
                other_era_id,
            } => {
                write!(
                    formatter,
                    "mismatch between era ids while merging block signatures - self: {}, other: \
                    {}",
                    self_era_id, other_era_id
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for BlockSignaturesMergeError {}

/// A collection of signatures for a single block, along with the associated block's hash and era
/// ID.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct BlockSignatures {
    /// The block hash.
    pub(super) block_hash: BlockHash,
    /// The era ID in which this block was created.
    pub(super) era_id: EraId,
    /// The proofs of the block, i.e. a collection of validators' signatures of the block hash.
    pub(super) proofs: BTreeMap<PublicKey, Signature>,
}

impl BlockSignatures {
    /// Constructs a new `BlockSignatures`.
    pub fn new(block_hash: BlockHash, era_id: EraId) -> Self {
        BlockSignatures {
            block_hash,
            era_id,
            proofs: BTreeMap::new(),
        }
    }

    /// Returns the block hash of the associated block.
    pub fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    /// Returns the era id of the associated block.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the finality signature associated with the given public key, if available.
    pub fn finality_signature(&self, public_key: &PublicKey) -> Option<FinalitySignature> {
        self.proofs
            .get(public_key)
            .map(|signature| FinalitySignature {
                block_hash: self.block_hash,
                era_id: self.era_id,
                signature: *signature,
                public_key: public_key.clone(),
                #[cfg(any(feature = "once_cell", test))]
                is_verified: OnceCell::new(),
            })
    }

    /// Returns `true` if there is a signature associated with the given public key.
    pub fn has_finality_signature(&self, public_key: &PublicKey) -> bool {
        self.proofs.contains_key(public_key)
    }

    /// Returns an iterator over all the signatures.
    pub fn finality_signatures(&self) -> impl Iterator<Item = FinalitySignature> + '_ {
        self.proofs
            .iter()
            .map(move |(public_key, signature)| FinalitySignature {
                block_hash: self.block_hash,
                era_id: self.era_id,
                signature: *signature,
                public_key: public_key.clone(),
                #[cfg(any(feature = "once_cell", test))]
                is_verified: OnceCell::new(),
            })
    }

    /// Returns an iterator over all the validator public keys.
    pub fn signers(&self) -> impl Iterator<Item = &'_ PublicKey> + '_ {
        self.proofs.keys()
    }

    /// Returns the number of signatures in the collection.
    pub fn len(&self) -> usize {
        self.proofs.len()
    }

    /// Returns `true` if there are no signatures in the collection.
    pub fn is_empty(&self) -> bool {
        self.proofs.is_empty()
    }

    /// Inserts a new signature.
    pub fn insert_signature(&mut self, finality_signature: FinalitySignature) {
        let _ = self
            .proofs
            .insert(finality_signature.public_key, finality_signature.signature);
    }

    /// Merges the collection of signatures in `other` into `self`.
    ///
    /// Returns an error if the block hashes or era IDs do not match.
    pub fn merge(&mut self, mut other: Self) -> Result<(), BlockSignaturesMergeError> {
        if self.block_hash != other.block_hash {
            return Err(BlockSignaturesMergeError::BlockHashMismatch {
                self_hash: self.block_hash,
                other_hash: other.block_hash,
            });
        }

        if self.era_id != other.era_id {
            return Err(BlockSignaturesMergeError::EraIdMismatch {
                self_era_id: self.era_id,
                other_era_id: other.era_id,
            });
        }

        self.proofs.append(&mut other.proofs);

        Ok(())
    }

    /// Returns `Ok` if and only if all the signatures are cryptographically valid.
    pub fn is_verified(&self) -> Result<(), crypto::Error> {
        for (public_key, signature) in self.proofs.iter() {
            let signature = FinalitySignature {
                block_hash: self.block_hash,
                era_id: self.era_id,
                signature: *signature,
                public_key: public_key.clone(),
                #[cfg(any(feature = "once_cell", test))]
                is_verified: OnceCell::new(),
            };
            signature.is_verified()?;
        }
        Ok(())
    }
}

impl Display for BlockSignatures {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "block signatures for {} in {} with {} proofs",
            self.block_hash,
            self.era_id,
            self.proofs.len()
        )
    }
}
