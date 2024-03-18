use alloc::{collections::BTreeSet, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

use super::Approval;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest,
};

/// The cryptographic hash of the bytesrepr-encoded set of approvals for a single [``].
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct ApprovalsHash(pub Digest);

impl ApprovalsHash {
    /// The number of bytes in a `ApprovalsHash` digest.
    pub const LENGTH: usize = Digest::LENGTH;

    /// Constructs a new `ApprovalsHash` by bytesrepr-encoding `approvals` and creating
    /// a [`Digest`] of this.
    pub fn compute(approvals: &BTreeSet<Approval>) -> Result<Self, bytesrepr::Error> {
        let digest = Digest::hash(approvals.to_bytes()?);
        Ok(ApprovalsHash(digest))
    }

    /// Returns the wrapped inner digest.
    pub fn inner(&self) -> &Digest {
        &self.0
    }

    /// Returns a new `ApprovalsHash` directly initialized with the provided bytes; no
    /// hashing is done.
    #[cfg(any(feature = "testing", test))]
    pub const fn from_raw(raw_digest: [u8; Self::LENGTH]) -> Self {
        ApprovalsHash(Digest::from_raw(raw_digest))
    }

    /// Returns a random `ApprovalsHash`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let hash = rng.gen::<[u8; Digest::LENGTH]>().into();
        ApprovalsHash(hash)
    }
}

impl From<ApprovalsHash> for Digest {
    fn from(hash: ApprovalsHash) -> Self {
        hash.0
    }
}

impl From<Digest> for ApprovalsHash {
    fn from(digest: Digest) -> Self {
        Self(digest)
    }
}

impl Display for ApprovalsHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "transaction-v1-approvals-hash({})", self.0,)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let hash = ApprovalsHash::random(rng);
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
