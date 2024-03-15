use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    Deploy, DeployHash, InvalidDeploy, Transaction,
};

use crate::components::fetcher::{EmptyValidationMetadata, FetchItem, Tag};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, DataSize, Debug)]
pub(crate) struct LegacyDeploy(Deploy);

impl FetchItem for LegacyDeploy {
    type Id = DeployHash;
    type ValidationError = InvalidDeploy;
    type ValidationMetadata = EmptyValidationMetadata;

    const TAG: Tag = Tag::LegacyDeploy;

    fn fetch_id(&self) -> Self::Id {
        *self.0.hash()
    }

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.0.has_valid_hash()
    }
}

impl ToBytes for LegacyDeploy {
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

impl FromBytes for LegacyDeploy {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Deploy::from_bytes(bytes).map(|(inner, remainder)| (LegacyDeploy(inner), remainder))
    }
}

impl From<LegacyDeploy> for Deploy {
    fn from(legacy_deploy: LegacyDeploy) -> Self {
        legacy_deploy.0
    }
}

impl From<LegacyDeploy> for Transaction {
    fn from(legacy_deploy: LegacyDeploy) -> Self {
        Self::Deploy(legacy_deploy.0)
    }
}

impl From<Deploy> for LegacyDeploy {
    fn from(deploy: Deploy) -> Self {
        Self(deploy)
    }
}

impl Display for LegacyDeploy {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "legacy-{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let legacy_deploy = LegacyDeploy::from(Deploy::random(&mut rng));
        bytesrepr::test_serialization_roundtrip(&legacy_deploy);
    }
}

mod specimen_support {
    use crate::utils::specimen::{Cache, LargestSpecimen, SizeEstimator};

    use super::LegacyDeploy;

    impl LargestSpecimen for LegacyDeploy {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            LegacyDeploy(LargestSpecimen::largest_specimen(estimator, cache))
        }
    }
}
