use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::TransactionV1Hash;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, PublicKey, SecretKey, Signature,
};

/// A struct containing a signature of a transaction hash and the public key of the signer.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TransactionV1Approval {
    signer: PublicKey,
    signature: Signature,
}

impl TransactionV1Approval {
    /// Creates an approval by signing the given transaction hash using the given secret key.
    pub fn create(hash: &TransactionV1Hash, secret_key: &SecretKey) -> Self {
        let signer = PublicKey::from(secret_key);
        let signature = crypto::sign(hash, secret_key, &signer);
        Self { signer, signature }
    }

    /// Returns a new approval.
    pub fn new(signer: PublicKey, signature: Signature) -> Self {
        Self { signer, signature }
    }

    /// Returns the public key of the approval's signer.
    pub fn signer(&self) -> &PublicKey {
        &self.signer
    }

    /// Returns the approval signature.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    /// Returns a random `TransactionV1Approval`.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = TransactionV1Hash::random(rng);
        let secret_key = SecretKey::random(rng);
        TransactionV1Approval::create(&hash, &secret_key)
    }
}

impl Display for TransactionV1Approval {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "approval({})", self.signer)
    }
}

impl ToBytes for TransactionV1Approval {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.signer.write_bytes(writer)?;
        self.signature.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.signer.serialized_length() + self.signature.serialized_length()
    }
}

impl FromBytes for TransactionV1Approval {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (signer, remainder) = PublicKey::from_bytes(bytes)?;
        let (signature, remainder) = Signature::from_bytes(remainder)?;
        let approval = TransactionV1Approval { signer, signature };
        Ok((approval, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let approval = TransactionV1Approval::random(rng);
        bytesrepr::test_serialization_roundtrip(&approval);
    }
}
