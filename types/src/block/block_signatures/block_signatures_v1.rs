use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use super::BlockHash;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, EraId, FinalitySignatureV1, PublicKey, Signature,
};

/// A collection of signatures for a single block, along with the associated block's hash and era
/// ID.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct BlockSignaturesV1 {
    /// The block hash.
    pub(super) block_hash: BlockHash,
    /// The era ID in which this block was created.
    pub(super) era_id: EraId,
    /// The proofs of the block, i.e. a collection of validators' signatures of the block hash.
    pub(super) proofs: BTreeMap<PublicKey, Signature>,
}

impl BlockSignaturesV1 {
    /// Constructs a new `BlockSignaturesV1`.
    pub fn new(block_hash: BlockHash, era_id: EraId) -> Self {
        BlockSignaturesV1 {
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
    pub fn finality_signature(&self, public_key: &PublicKey) -> Option<FinalitySignatureV1> {
        self.proofs
            .get(public_key)
            .map(|signature| FinalitySignatureV1 {
                block_hash: self.block_hash,
                era_id: self.era_id,
                signature: *signature,
                public_key: public_key.clone(),
                #[cfg(any(feature = "once_cell", test))]
                is_verified: Default::default(),
            })
    }

    /// Returns `true` if there is a signature associated with the given public key.
    pub fn has_finality_signature(&self, public_key: &PublicKey) -> bool {
        self.proofs.contains_key(public_key)
    }

    /// Returns an iterator over all the signatures.
    pub fn finality_signatures(&self) -> impl Iterator<Item = FinalitySignatureV1> + '_ {
        self.proofs
            .iter()
            .map(move |(public_key, signature)| FinalitySignatureV1 {
                block_hash: self.block_hash,
                era_id: self.era_id,
                signature: *signature,
                public_key: public_key.clone(),
                #[cfg(any(feature = "once_cell", test))]
                is_verified: Default::default(),
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
    pub fn insert_signature(&mut self, public_key: PublicKey, signature: Signature) {
        let _ = self.proofs.insert(public_key, signature);
    }

    /// Returns `Ok` if and only if all the signatures are cryptographically valid.
    pub fn is_verified(&self) -> Result<(), crypto::Error> {
        for (public_key, signature) in self.proofs.iter() {
            let signature = FinalitySignatureV1 {
                block_hash: self.block_hash,
                era_id: self.era_id,
                signature: *signature,
                public_key: public_key.clone(),
                #[cfg(any(feature = "once_cell", test))]
                is_verified: Default::default(),
            };
            signature.is_verified()?;
        }
        Ok(())
    }

    /// Returns a random `BlockSignaturesV1`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let block_hash = BlockHash::random(rng);
        let era_id = EraId::random(rng);
        let proofs = (0..rng.gen_range(0..10))
            .map(|_| {
                let public_key = PublicKey::random(rng);
                let bytes = std::array::from_fn(|_| rng.gen());
                let signature = Signature::ed25519(bytes).unwrap();
                (public_key, signature)
            })
            .collect();
        Self {
            block_hash,
            era_id,
            proofs,
        }
    }
}

impl Display for BlockSignaturesV1 {
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

impl ToBytes for BlockSignaturesV1 {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buf = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buf)?;
        Ok(buf)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.block_hash.write_bytes(writer)?;
        self.era_id.write_bytes(writer)?;
        self.proofs.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.block_hash.serialized_length()
            + self.era_id.serialized_length()
            + self.proofs.serialized_length()
    }
}

impl FromBytes for BlockSignaturesV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (era_id, remainder) = EraId::from_bytes(remainder)?;
        let (proofs, remainder) = BTreeMap::<PublicKey, Signature>::from_bytes(remainder)?;
        Ok((
            Self {
                block_hash,
                era_id,
                proofs,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let hash = BlockSignaturesV1::random(rng);
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}