use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::TransactionV1;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest,
};

/// The cryptographic hash of a [`TransactionV1`].
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Hex-encoded TransactionV1 hash.")
)]
#[serde(deny_unknown_fields)]
pub struct TransactionV1Hash(Digest);

impl TransactionV1Hash {
    /// The number of bytes in a `TransactionV1Hash` digest.
    pub const LENGTH: usize = Digest::LENGTH;

    /// Constructs a new `TransactionV1Hash`.
    pub const fn new(hash: Digest) -> Self {
        TransactionV1Hash(hash)
    }

    /// Returns the wrapped inner digest.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Returns a new `TransactionV1Hash` directly initialized with the provided bytes; no hashing
    /// is done.
    #[cfg(any(feature = "testing", test))]
    pub const fn from_raw(raw_digest: [u8; Self::LENGTH]) -> Self {
        TransactionV1Hash(Digest::from_raw(raw_digest))
    }

    /// Returns a random `TransactionV1Hash`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        TransactionV1Hash(hash)
    }
}

impl From<Digest> for TransactionV1Hash {
    fn from(digest: Digest) -> Self {
        TransactionV1Hash(digest)
    }
}

impl From<TransactionV1Hash> for Digest {
    fn from(transaction_hash: TransactionV1Hash) -> Self {
        transaction_hash.0
    }
}

impl Display for TransactionV1Hash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "transaction-v1-hash({})", self.0)
    }
}

impl AsRef<[u8]> for TransactionV1Hash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl ToBytes for TransactionV1Hash {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for TransactionV1Hash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Digest::from_bytes(bytes).map(|(inner, remainder)| (TransactionV1Hash(inner), remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let hash = TransactionV1Hash::random(rng);
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
