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
use super::Deploy;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest,
};

/// The cryptographic hash of a [`Deploy`].
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Hex-encoded deploy hash.")
)]
#[serde(deny_unknown_fields)]
pub struct DeployHash(Digest);

impl DeployHash {
    /// The number of bytes in a `DeployHash` digest.
    pub const LENGTH: usize = Digest::LENGTH;

    /// Constructs a new `DeployHash`.
    pub const fn new(hash: Digest) -> Self {
        DeployHash(hash)
    }

    /// Returns the wrapped inner digest.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Returns a new `DeployHash` directly initialized with the provided bytes; no hashing is done.
    #[cfg(any(feature = "testing", test))]
    pub const fn from_raw(raw_digest: [u8; Self::LENGTH]) -> Self {
        DeployHash(Digest::from_raw(raw_digest))
    }

    /// Returns a random `DeployHash`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        DeployHash(hash)
    }
}

impl From<Digest> for DeployHash {
    fn from(digest: Digest) -> Self {
        DeployHash(digest)
    }
}

impl From<DeployHash> for Digest {
    fn from(deploy_hash: DeployHash) -> Self {
        deploy_hash.0
    }
}

impl Display for DeployHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "deploy-hash({})", self.0,)
    }
}

impl AsRef<[u8]> for DeployHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl ToBytes for DeployHash {
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

impl FromBytes for DeployHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Digest::from_bytes(bytes).map(|(inner, remainder)| (DeployHash(inner), remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let hash = DeployHash::random(rng);
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
