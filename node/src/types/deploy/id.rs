use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use casper_types::testing::TestRng;

use super::{ApprovalsHash, DeployHash};

/// The unique identifier of a deploy, comprising its [`DeployHash`] and [`ApprovalsHash`].
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
pub struct Id {
    deploy_hash: DeployHash,
    approvals_hash: ApprovalsHash,
}

impl Id {
    /// Returns a new ID.
    pub fn new(deploy_hash: DeployHash, approvals_hash: ApprovalsHash) -> Self {
        Id {
            deploy_hash,
            approvals_hash,
        }
    }

    /// Returns the deploy hash.
    pub fn deploy_hash(&self) -> &DeployHash {
        &self.deploy_hash
    }

    /// Returns the approvals hash.
    pub fn approvals_hash(&self) -> &ApprovalsHash {
        &self.approvals_hash
    }

    pub fn destructure(self) -> (DeployHash, ApprovalsHash) {
        (self.deploy_hash, self.approvals_hash)
    }

    /// Returns a random `ApprovalsHash`.
    #[allow(unused)]
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        Id::new(DeployHash::random(rng), ApprovalsHash::random(rng))
    }
}

impl Display for Id {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy-id({}, {})",
            self.deploy_hash, self.approvals_hash
        )
    }
}

impl ToBytes for Id {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.deploy_hash.write_bytes(writer)?;
        self.approvals_hash.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.deploy_hash.serialized_length() + self.approvals_hash.serialized_length()
    }
}

impl FromBytes for Id {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (deploy_hash, remainder) = DeployHash::from_bytes(bytes)?;
        let (approvals_hash, remainder) = ApprovalsHash::from_bytes(remainder)?;
        let id = Id::new(deploy_hash, approvals_hash);
        Ok((id, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let id = Id::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&id);
    }
}
