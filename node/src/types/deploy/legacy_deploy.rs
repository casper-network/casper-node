use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use super::{Deploy, DeployConfigurationFailure, DeployHash};
use crate::types::{EmptyValidationMetadata, FetcherItem, Item, Tag};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, DataSize, Debug)]
pub(crate) struct LegacyDeploy(Deploy);

impl LegacyDeploy {
    #[cfg(test)]
    pub(crate) fn inner(&self) -> &Deploy {
        &self.0
    }
}

impl Item for LegacyDeploy {
    type Id = DeployHash;

    fn id(&self) -> Self::Id {
        *self.0.hash()
    }
}

impl FetcherItem for LegacyDeploy {
    type ValidationError = DeployConfigurationFailure;
    type ValidationMetadata = EmptyValidationMetadata;
    const TAG: Tag = Tag::LegacyDeploy;

    fn validate(&self, metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.0.validate(metadata)
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
