use std::{
    collections::BTreeSet,
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

use casper_hashing::Digest;
use casper_types::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use casper_types::testing::TestRng;

use super::Approval;

/// The cryptographic hash of the bytesrepr-encoded set of approvals for a single deploy.
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
)]
#[serde(deny_unknown_fields)]
pub struct ApprovalsHash(Digest);

impl ApprovalsHash {
    /// Constructs a new `ApprovalsHash` by bytesrepr-encoding `approvals` and creating a [`Digest`]
    /// of this.
    pub fn compute(approvals: &BTreeSet<Approval>) -> Result<Self, bytesrepr::Error> {
        let digest = Digest::hash(approvals.to_bytes()?);
        Ok(ApprovalsHash(digest))
    }

    /// Returns the wrapped inner hash digest.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Returns a random `ApprovalsHash`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        ApprovalsHash(hash)
    }
}

impl Display for ApprovalsHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "approvals-hash({})", self.0,)
    }
}

impl AsRef<[u8]> for ApprovalsHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl ToBytes for ApprovalsHash {
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

impl FromBytes for ApprovalsHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Digest::from_bytes(bytes).map(|(inner, remainder)| (ApprovalsHash(inner), remainder))
    }
}

impl From<ApprovalsHash> for Digest {
    fn from(deploy_hash: ApprovalsHash) -> Self {
        deploy_hash.0
    }
}

impl From<Digest> for ApprovalsHash {
    fn from(digest: Digest) -> Self {
        Self(digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let hash = ApprovalsHash::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
