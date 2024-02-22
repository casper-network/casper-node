mod block_signatures_v1;
mod block_signatures_v2;

pub use block_signatures_v1::BlockSignaturesV1;
pub use block_signatures_v2::BlockSignaturesV2;

use alloc::{collections::BTreeMap, vec::Vec};
use core::{
    fmt::{self, Display, Formatter},
    hash::Hash,
};
use itertools::Either;
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    crypto, BlockHash, ChainNameDigest, EraId, FinalitySignature, PublicKey, Signature,
};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for block signatures v1.
pub const BLOCK_SIGNATURES_V1_TAG: u8 = 0;
/// Tag for block signatures v2.
pub const BLOCK_SIGNATURES_V2_TAG: u8 = 1;

/// A collection of signatures for a single block, along with the associated block's hash and era
/// ID.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum BlockSignatures {
    /// Version 1 of the block signatures.
    V1(BlockSignaturesV1),
    /// Version 2 of the block signatures.
    V2(BlockSignaturesV2),
}

impl BlockSignatures {
    /// Returns the block hash of the associated block.
    pub fn block_hash(&self) -> &BlockHash {
        match self {
            BlockSignatures::V1(block_signatures) => block_signatures.block_hash(),
            BlockSignatures::V2(block_signatures) => block_signatures.block_hash(),
        }
    }

    /// Returns the era id of the associated block.
    pub fn era_id(&self) -> EraId {
        match self {
            BlockSignatures::V1(block_signatures) => block_signatures.era_id(),
            BlockSignatures::V2(block_signatures) => block_signatures.era_id(),
        }
    }

    /// Returns the finality signature associated with the given public key, if available.
    pub fn finality_signature(&self, public_key: &PublicKey) -> Option<FinalitySignature> {
        match self {
            BlockSignatures::V1(block_signatures) => block_signatures
                .finality_signature(public_key)
                .map(FinalitySignature::V1),
            BlockSignatures::V2(block_signatures) => block_signatures
                .finality_signature(public_key)
                .map(FinalitySignature::V2),
        }
    }

    /// Returns `true` if there is a signature associated with the given public key.
    pub fn has_finality_signature(&self, public_key: &PublicKey) -> bool {
        match self {
            BlockSignatures::V1(block_signatures) => {
                block_signatures.has_finality_signature(public_key)
            }
            BlockSignatures::V2(block_signatures) => {
                block_signatures.has_finality_signature(public_key)
            }
        }
    }

    /// Returns an iterator over all the signatures.
    pub fn finality_signatures(&self) -> impl Iterator<Item = FinalitySignature> + '_ {
        match self {
            BlockSignatures::V1(block_signatures) => Either::Left(
                block_signatures
                    .finality_signatures()
                    .map(FinalitySignature::V1),
            ),
            BlockSignatures::V2(block_signatures) => Either::Right(
                block_signatures
                    .finality_signatures()
                    .map(FinalitySignature::V2),
            ),
        }
    }

    /// Returns an `BTreeMap` of public keys to signatures.
    pub fn proofs(&self) -> &BTreeMap<PublicKey, Signature> {
        match self {
            BlockSignatures::V1(block_signatures) => &block_signatures.proofs,
            BlockSignatures::V2(block_signatures) => &block_signatures.proofs,
        }
    }

    /// Returns an iterator over all the validator public keys.
    pub fn signers(&self) -> impl Iterator<Item = &'_ PublicKey> + '_ {
        match self {
            BlockSignatures::V1(block_signatures) => Either::Left(block_signatures.signers()),
            BlockSignatures::V2(block_signatures) => Either::Right(block_signatures.signers()),
        }
    }

    /// Returns the number of signatures in the collection.
    pub fn len(&self) -> usize {
        match self {
            BlockSignatures::V1(block_signatures) => block_signatures.len(),
            BlockSignatures::V2(block_signatures) => block_signatures.len(),
        }
    }

    /// Returns `true` if there are no signatures in the collection.
    pub fn is_empty(&self) -> bool {
        match self {
            BlockSignatures::V1(block_signatures) => block_signatures.is_empty(),
            BlockSignatures::V2(block_signatures) => block_signatures.is_empty(),
        }
    }

    /// Merges the collection of signatures in `other` into `self`.
    ///
    /// Returns an error if the block hashes, block heights, era IDs, or chain name hashes do not
    /// match.
    pub fn merge(&mut self, mut other: Self) -> Result<(), BlockSignaturesMergeError> {
        if self.block_hash() != other.block_hash() {
            return Err(BlockSignaturesMergeError::BlockHashMismatch {
                self_hash: *self.block_hash(),
                other_hash: *other.block_hash(),
            });
        }

        if self.era_id() != other.era_id() {
            return Err(BlockSignaturesMergeError::EraIdMismatch {
                self_era_id: self.era_id(),
                other_era_id: other.era_id(),
            });
        }

        match (self, &mut other) {
            (BlockSignatures::V1(self_), BlockSignatures::V1(other)) => {
                self_.proofs.append(&mut other.proofs);
            }
            (BlockSignatures::V2(self_), BlockSignatures::V2(other)) => {
                if self_.block_height != other.block_height {
                    return Err(BlockSignaturesMergeError::BlockHeightMismatch {
                        self_height: self_.block_height,
                        other_height: other.block_height,
                    });
                }

                if self_.chain_name_hash != other.chain_name_hash {
                    return Err(BlockSignaturesMergeError::ChainNameHashMismatch {
                        self_chain_name_hash: self_.chain_name_hash,
                        other_chain_name_hash: other.chain_name_hash,
                    });
                }

                self_.proofs.append(&mut other.proofs);
            }
            _ => return Err(BlockSignaturesMergeError::VersionMismatch),
        }

        Ok(())
    }

    /// Returns `Ok` if and only if all the signatures are cryptographically valid.
    pub fn is_verified(&self) -> Result<(), crypto::Error> {
        match self {
            BlockSignatures::V1(block_signatures) => block_signatures.is_verified(),
            BlockSignatures::V2(block_signatures) => block_signatures.is_verified(),
        }
    }

    /// Converts self into a `BTreeMap` of public keys to signatures.
    pub fn into_proofs(self) -> BTreeMap<PublicKey, Signature> {
        match self {
            BlockSignatures::V1(block_signatures) => block_signatures.proofs,
            BlockSignatures::V2(block_signatures) => block_signatures.proofs,
        }
    }

    /// Inserts a new signature.
    pub fn insert_signature(&mut self, public_key: PublicKey, signature: Signature) {
        match self {
            BlockSignatures::V1(block_signatures) => {
                block_signatures.insert_signature(public_key, signature)
            }
            BlockSignatures::V2(block_signatures) => {
                block_signatures.insert_signature(public_key, signature)
            }
        }
    }

    /// Sets the era ID to its max value, rendering it and hence `self` invalid (assuming the
    /// relevant era ID for this `SignedBlockHeader` wasn't already the max value).
    #[cfg(any(feature = "testing", test))]
    pub fn invalidate_era(&mut self) {
        match self {
            BlockSignatures::V1(block_signatures) => block_signatures.era_id = EraId::new(u64::MAX),
            BlockSignatures::V2(block_signatures) => block_signatures.era_id = EraId::new(u64::MAX),
        }
    }

    /// Replaces the signature field of the last `proofs` entry with the `System` variant
    /// of [`Signature`], rendering that entry invalid.
    #[cfg(any(feature = "testing", test))]
    pub fn invalidate_last_signature(&mut self) {
        let proofs = match self {
            BlockSignatures::V1(block_signatures) => &mut block_signatures.proofs,
            BlockSignatures::V2(block_signatures) => &mut block_signatures.proofs,
        };
        let last_proof = proofs
            .last_entry()
            .expect("should have at least one signature");
        *last_proof.into_mut() = Signature::System;
    }

    /// Returns a random `BlockSignatures`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            BlockSignatures::V1(BlockSignaturesV1::random(rng))
        } else {
            BlockSignatures::V2(BlockSignaturesV2::random(rng))
        }
    }
}

impl Display for BlockSignatures {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            BlockSignatures::V1(block_signatures) => write!(formatter, "{}", block_signatures),
            BlockSignatures::V2(block_signatures) => write!(formatter, "{}", block_signatures),
        }
    }
}

impl From<BlockSignaturesV1> for BlockSignatures {
    fn from(block_signatures: BlockSignaturesV1) -> Self {
        BlockSignatures::V1(block_signatures)
    }
}

impl From<BlockSignaturesV2> for BlockSignatures {
    fn from(block_signatures: BlockSignaturesV2) -> Self {
        BlockSignatures::V2(block_signatures)
    }
}

impl ToBytes for BlockSignatures {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buf = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buf)?;
        Ok(buf)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            BlockSignatures::V1(block_signatures) => {
                writer.push(BLOCK_SIGNATURES_V1_TAG);
                block_signatures.write_bytes(writer)?;
            }
            BlockSignatures::V2(block_signatures) => {
                writer.push(BLOCK_SIGNATURES_V2_TAG);
                block_signatures.write_bytes(writer)?;
            }
        }
        Ok(())
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                BlockSignatures::V1(block_signatures) => block_signatures.serialized_length(),
                BlockSignatures::V2(block_signatures) => block_signatures.serialized_length(),
            }
    }
}

impl FromBytes for BlockSignatures {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            BLOCK_SIGNATURES_V1_TAG => {
                let (block_signatures, remainder) = BlockSignaturesV1::from_bytes(remainder)?;
                Ok((BlockSignatures::V1(block_signatures), remainder))
            }
            BLOCK_SIGNATURES_V2_TAG => {
                let (block_signatures, remainder) = BlockSignaturesV2::from_bytes(remainder)?;
                Ok((BlockSignatures::V2(block_signatures), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// An error returned during an attempt to merge two incompatible [`BlockSignaturesV1`].
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
    /// A mismatch between block heights.
    BlockHeightMismatch {
        /// The `self` height.
        self_height: u64,
        /// The `other` height.
        other_height: u64,
    },
    /// A mismatch between era IDs.
    EraIdMismatch {
        /// The `self` era ID.
        self_era_id: EraId,
        /// The `other` era ID.
        other_era_id: EraId,
    },
    /// A mismatch between chain name hashes.
    ChainNameHashMismatch {
        /// The `self` chain name hash.
        self_chain_name_hash: ChainNameDigest,
        /// The `other` chain name hash.
        other_chain_name_hash: ChainNameDigest,
    },
    /// A mismatch between the versions of the block signatures.
    VersionMismatch,
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
            BlockSignaturesMergeError::BlockHeightMismatch {
                self_height,
                other_height,
            } => {
                write!(
                    formatter,
                    "mismatch between block heights while merging block signatures - self: {}, \
                    other: {}",
                    self_height, other_height
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
            BlockSignaturesMergeError::ChainNameHashMismatch {
                self_chain_name_hash,
                other_chain_name_hash,
            } => {
                write!(
                    formatter,
                    "mismatch between chain name hashes while merging block signatures - self: {}, \
                    other: {}",
                    self_chain_name_hash, other_chain_name_hash
                )
            }
            BlockSignaturesMergeError::VersionMismatch => {
                write!(
                    formatter,
                    "mismatch between versions of block signatures while merging"
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for BlockSignaturesMergeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let hash = BlockSignatures::random(rng);
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
