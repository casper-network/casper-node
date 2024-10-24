use alloc::{string::String, vec::Vec};
use core::convert::TryFrom;

use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::Digest;

pub(crate) mod raw_32_byte_array {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(
        array: &[u8; 32],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            base16::encode_lower(array).serialize(serializer)
        } else {
            array.serialize(serializer)
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; 32], D::Error> {
        if deserializer.is_human_readable() {
            let hex_string = String::deserialize(deserializer)?;
            let bytes = base16::decode(hex_string.as_bytes()).map_err(SerdeError::custom)?;
            <[u8; 32]>::try_from(bytes.as_ref()).map_err(SerdeError::custom)
        } else {
            <[u8; 32]>::deserialize(deserializer)
        }
    }
}

pub(crate) mod contract_hash_as_digest {
    use super::*;
    use crate::contracts::ContractHash;

    pub(crate) fn serialize<S: Serializer>(
        contract_hash: &ContractHash,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Digest::from(contract_hash.value()).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<ContractHash, D::Error> {
        let digest = Digest::deserialize(deserializer)?;
        Ok(ContractHash::new(digest.value()))
    }
}

pub(crate) mod contract_package_hash_as_digest {
    use super::*;
    use crate::contracts::ContractPackageHash;

    pub(crate) fn serialize<S: Serializer>(
        contract_package_hash: &ContractPackageHash,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Digest::from(contract_package_hash.value()).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<ContractPackageHash, D::Error> {
        let digest = Digest::deserialize(deserializer)?;
        Ok(ContractPackageHash::new(digest.value()))
    }
}

/// This module allows `DeployHash`es to be serialized and deserialized using the underlying
/// `[u8; 32]` rather than delegating to the wrapped `Digest`, which in turn delegates to a
/// `Vec<u8>` for legacy reasons.
///
/// This is required as the `DeployHash` defined in `casper-types` up until v4.0.0 used the array
/// form, while the `DeployHash` defined in `casper-node` during this period delegated to `Digest`.
///
/// We use this module in places where the old `casper_types::DeployHash` was held as a member of a
/// type which implements `Serialize` and/or `Deserialize`.
pub(crate) mod deploy_hash_as_array {
    use super::*;
    use crate::DeployHash;

    pub(crate) fn serialize<S: Serializer>(
        deploy_hash: &DeployHash,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            base16::encode_lower(&deploy_hash.inner().value()).serialize(serializer)
        } else {
            deploy_hash.inner().value().serialize(serializer)
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DeployHash, D::Error> {
        let bytes = if deserializer.is_human_readable() {
            let hex_string = String::deserialize(deserializer)?;
            let vec_bytes = base16::decode(hex_string.as_bytes()).map_err(SerdeError::custom)?;
            <[u8; DeployHash::LENGTH]>::try_from(vec_bytes.as_ref()).map_err(SerdeError::custom)?
        } else {
            <[u8; DeployHash::LENGTH]>::deserialize(deserializer)?
        };
        Ok(DeployHash::new(Digest::from(bytes)))
    }
}

pub mod contract_package {
    use core::convert::TryFrom;

    use super::*;
    #[cfg(feature = "json-schema")]
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    use crate::{
        contracts::{
            ContractHash, ContractPackage, ContractPackageStatus, ContractVersion,
            ContractVersionKey, ContractVersions, DisabledVersions, ProtocolVersionMajor,
        },
        Groups, URef,
    };

    #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[cfg_attr(feature = "json-schema", derive(JsonSchema))]
    #[cfg_attr(feature = "json-schema", schemars(rename = "ContractVersion"))]
    pub struct HumanReadableContractVersion {
        protocol_version_major: ProtocolVersionMajor,
        contract_version: ContractVersion,
        contract_hash: ContractHash,
    }

    /// Helper struct for deserializing/serializing `ContractPackage` from and to JSON.
    #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[cfg_attr(feature = "json-schema", derive(JsonSchema))]
    #[cfg_attr(feature = "json-schema", schemars(rename = "ContractPackage"))]
    pub struct HumanReadableContractPackage {
        access_key: URef,
        versions: Vec<HumanReadableContractVersion>,
        disabled_versions: DisabledVersions,
        groups: Groups,
        lock_status: ContractPackageStatus,
    }

    impl From<&ContractPackage> for HumanReadableContractPackage {
        fn from(package: &ContractPackage) -> Self {
            let mut versions = vec![];
            for (key, hash) in package.versions() {
                versions.push(HumanReadableContractVersion {
                    protocol_version_major: key.protocol_version_major(),
                    contract_version: key.contract_version(),
                    contract_hash: *hash,
                });
            }
            HumanReadableContractPackage {
                access_key: package.access_key(),
                versions,
                disabled_versions: package.disabled_versions().clone(),
                groups: package.groups().clone(),
                lock_status: package.lock_status(),
            }
        }
    }

    impl TryFrom<HumanReadableContractPackage> for ContractPackage {
        type Error = String;

        fn try_from(value: HumanReadableContractPackage) -> Result<Self, Self::Error> {
            let mut versions = ContractVersions::default();
            for version in value.versions.iter() {
                let key = ContractVersionKey::new(
                    version.protocol_version_major,
                    version.contract_version,
                );
                if versions.contains_key(&key) {
                    return Err(format!("duplicate contract version: {:?}", key));
                }
                versions.insert(key, version.contract_hash);
            }
            Ok(ContractPackage::new(
                value.access_key,
                versions,
                value.disabled_versions,
                value.groups,
                value.lock_status,
            ))
        }
    }
}
