use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Deploy;
use super::{DeployApprovalsHash, DeployHash};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    TransactionId,
};

/// The unique identifier of a [`Deploy`], comprising its [`DeployHash`] and
/// [`DeployApprovalsHash`].
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct DeployId {
    deploy_hash: DeployHash,
    approvals_hash: DeployApprovalsHash,
}

impl DeployId {
    /// Returns a new `DeployId`.
    pub fn new(deploy_hash: DeployHash, approvals_hash: DeployApprovalsHash) -> Self {
        DeployId {
            deploy_hash,
            approvals_hash,
        }
    }

    /// Returns the deploy hash.
    pub fn deploy_hash(&self) -> &DeployHash {
        &self.deploy_hash
    }

    /// Returns the approvals hash.
    pub fn approvals_hash(&self) -> &DeployApprovalsHash {
        &self.approvals_hash
    }

    /// Consumes `self`, returning a tuple of the constituent parts.
    pub fn destructure(self) -> (DeployHash, DeployApprovalsHash) {
        (self.deploy_hash, self.approvals_hash)
    }

    /// Returns a random `DeployId`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        DeployId::new(DeployHash::random(rng), DeployApprovalsHash::random(rng))
    }
}

impl Display for DeployId {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy-id({}, {})",
            self.deploy_hash, self.approvals_hash
        )
    }
}

impl ToBytes for DeployId {
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

impl FromBytes for DeployId {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (deploy_hash, remainder) = DeployHash::from_bytes(bytes)?;
        let (approvals_hash, remainder) = DeployApprovalsHash::from_bytes(remainder)?;
        let id = DeployId::new(deploy_hash, approvals_hash);
        Ok((id, remainder))
    }
}

impl From<DeployId> for TransactionId {
    fn from(id: DeployId) -> Self {
        Self::Deploy {
            deploy_hash: id.deploy_hash,
            approvals_hash: id.approvals_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let id = DeployId::random(rng);
        bytesrepr::test_serialization_roundtrip(&id);
    }
}
