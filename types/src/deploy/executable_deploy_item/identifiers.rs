use alloc::{string::String, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use hex_fmt::HexFmt;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    ContractHash, ContractPackageHash, ContractVersion,
};
#[cfg(doc)]
use crate::{DirectCallV1, ExecutableDeployItem};

const CONTRACT_PACKAGE_ID_HASH_TAG: u8 = 0;
const CONTRACT_PACKAGE_ID_NAME_TAG: u8 = 1;

/// Identifier for an [`ExecutableDeployItem`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExecutableDeployItemIdentifier {
    /// The deploy item is of the type [`ExecutableDeployItem::ModuleBytes`]
    Module,
    /// The deploy item is a variation of a stored contract.
    Contract(ContractIdentifier),
    /// The deploy item is a variation of a stored contract package.
    Package(ContractPackageIdentifier),
    /// The deploy item is a native transfer.
    Transfer,
}

/// Identifier for the contract object within a [`DirectCallV1`] or an [`ExecutableDeployItem`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ContractIdentifier {
    /// The contract object within the deploy item is identified by its hash.
    Hash(ContractHash),
    /// The contract object within the deploy item is identified by name.
    Name(String),
}

/// Identifier for the contract package object within a [`DirectCallV1`] or an
/// [`ExecutableDeployItem`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Identifier for a contract package.")
)]
pub enum ContractPackageIdentifier {
    /// The stored contract package within the deploy item is identified by its hash.
    Hash {
        /// Hash of the contract package.
        contract_package_hash: ContractPackageHash,
        /// The version specified in the deploy item.
        version: Option<ContractVersion>,
    },
    /// The stored contract package within the deploy item is identified by name.
    Name {
        /// Name of the contract package.
        name: String,
        /// The version specified in the deploy item.
        version: Option<ContractVersion>,
    },
}

impl ContractPackageIdentifier {
    /// Returns the version of the contract package specified in the deploy item.
    pub fn version(&self) -> Option<ContractVersion> {
        match self {
            ContractPackageIdentifier::Hash { version, .. }
            | ContractPackageIdentifier::Name { version, .. } => *version,
        }
    }

    /// Returns a random `ContractPackageIdentifier`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let version = rng.gen::<bool>().then(|| rng.gen::<ContractVersion>());
        if rng.gen() {
            ContractPackageIdentifier::Hash {
                contract_package_hash: ContractPackageHash::new(rng.gen()),
                version,
            }
        } else {
            ContractPackageIdentifier::Name {
                name: rng.random_string(1..21),
                version,
            }
        }
    }
}

impl Display for ContractPackageIdentifier {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            ContractPackageIdentifier::Hash {
                contract_package_hash,
                version: Some(ver),
            } => write!(
                formatter,
                "contract-package-id(hash: {}, version: {})",
                HexFmt(contract_package_hash),
                ver
            ),
            ContractPackageIdentifier::Hash {
                contract_package_hash,
                ..
            } => write!(
                formatter,
                "contract-package-id(hash: {}, version: latest)",
                HexFmt(contract_package_hash),
            ),
            ContractPackageIdentifier::Name {
                name,
                version: Some(ver),
            } => write!(
                formatter,
                "contract-package-id(name: {}, version: {})",
                name, ver
            ),
            ContractPackageIdentifier::Name { name, .. } => write!(
                formatter,
                "contract-package-id(name: {}, version: latest)",
                name
            ),
        }
    }
}

impl ToBytes for ContractPackageIdentifier {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ContractPackageIdentifier::Hash {
                contract_package_hash,
                version,
            } => {
                CONTRACT_PACKAGE_ID_HASH_TAG.write_bytes(writer)?;
                contract_package_hash.write_bytes(writer)?;
                version.write_bytes(writer)
            }
            ContractPackageIdentifier::Name { name, version } => {
                CONTRACT_PACKAGE_ID_NAME_TAG.write_bytes(writer)?;
                name.write_bytes(writer)?;
                version.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                ContractPackageIdentifier::Hash {
                    contract_package_hash,
                    version,
                } => contract_package_hash.serialized_length() + version.serialized_length(),
                ContractPackageIdentifier::Name { name, version } => {
                    name.serialized_length() + version.serialized_length()
                }
            }
    }
}

impl FromBytes for ContractPackageIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            CONTRACT_PACKAGE_ID_HASH_TAG => {
                let (contract_package_hash, remainder) =
                    ContractPackageHash::from_bytes(remainder)?;
                let (version, remainder) = Option::<ContractVersion>::from_bytes(remainder)?;
                let id = ContractPackageIdentifier::Hash {
                    contract_package_hash,
                    version,
                };
                Ok((id, remainder))
            }
            CONTRACT_PACKAGE_ID_NAME_TAG => {
                let (name, remainder) = String::from_bytes(remainder)?;
                let (version, remainder) = Option::<ContractVersion>::from_bytes(remainder)?;
                let id = ContractPackageIdentifier::Name { name, version };
                Ok((id, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&ContractPackageIdentifier::random(rng));
    }
}
