use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_hashing::Digest;
use casper_types::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use casper_types::testing::TestRng;

/// The cryptographic hash of a [`Deploy`](struct.Deploy.html).
#[derive(
    Copy,
    Clone,
    DataSize,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Debug,
    Default,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
#[schemars(with = "String", description = "Hex-encoded deploy hash.")]
pub struct DeployHash(#[schemars(skip)] Digest);

impl DeployHash {
    /// Constructs a new `DeployHash`.
    pub fn new(hash: Digest) -> Self {
        DeployHash(hash)
    }

    /// Returns the wrapped inner hash.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Returns a random `DeployHash`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        DeployHash(hash)
    }
}

impl From<DeployHash> for casper_types::DeployHash {
    fn from(deploy_hash: DeployHash) -> casper_types::DeployHash {
        casper_types::DeployHash::new(deploy_hash.inner().value())
    }
}

impl From<casper_types::DeployHash> for DeployHash {
    fn from(deploy_hash: casper_types::DeployHash) -> DeployHash {
        DeployHash::new(deploy_hash.value().into())
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

impl From<Digest> for DeployHash {
    fn from(digest: Digest) -> Self {
        Self(digest)
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
        let mut rng = crate::new_rng();
        let hash = DeployHash(rng.gen::<[u8; Digest::LENGTH]>().into());
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
