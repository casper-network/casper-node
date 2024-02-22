use core::fmt::{self, Display, Formatter};

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest,
};

/// A cryptographic hash of a chain name.
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Hex-encoded cryptographic hash of a chain name.")
)]
#[serde(deny_unknown_fields)]
pub struct ChainNameDigest(Digest);

impl ChainNameDigest {
    /// The number of bytes in a `ChainNameDigest` digest.
    pub const LENGTH: usize = Digest::LENGTH;

    /// Constructs a new `ChainNameDigest` from the given chain name.
    pub fn from_chain_name(name: &str) -> Self {
        ChainNameDigest(Digest::hash(name.as_bytes()))
    }

    /// Returns the wrapped inner digest.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Returns a new `ChainNameDigest` directly initialized with the provided `Digest`;
    /// no hashing is done.
    #[cfg(any(feature = "testing", test))]
    pub const fn from_digest(digest: Digest) -> Self {
        ChainNameDigest(digest)
    }

    /// Returns a random `ChainNameDigest`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        ChainNameDigest(hash)
    }
}

impl Display for ChainNameDigest {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "chain-name-hash({})", self.0)
    }
}

impl ToBytes for ChainNameDigest {
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

impl FromBytes for ChainNameDigest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Digest::from_bytes(bytes).map(|(inner, remainder)| (Self(inner), remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let hash = ChainNameDigest::random(rng);
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
