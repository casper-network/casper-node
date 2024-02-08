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
    EntityVersion, PackageHash,
};
#[cfg(doc)]
use crate::{ExecutableDeployItem, TransactionTarget};

const HASH_TAG: u8 = 0;
const NAME_TAG: u8 = 1;

/// Identifier for the package object within a [`TransactionTarget::Stored`] or an
/// [`ExecutableDeployItem`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(
        description = "Identifier for the package object within a `Stored` transaction target or \
        an `ExecutableDeployItem`."
    )
)]
pub enum PackageIdentifier {
    /// The hash and optional version identifying the contract package.
    Hash {
        /// The hash of the contract package.
        package_hash: PackageHash,
        /// The version of the contract package.
        ///
        /// `None` implies latest version.
        version: Option<EntityVersion>,
    },
    /// The name and optional version identifying the contract package.
    Name {
        /// The name of the contract package.
        name: String,
        /// The version of the contract package.
        ///
        /// `None` implies latest version.
        version: Option<EntityVersion>,
    },
}

impl PackageIdentifier {
    /// Returns the optional version of the contract package.
    ///
    /// `None` implies latest version.
    pub fn version(&self) -> Option<EntityVersion> {
        match self {
            PackageIdentifier::Hash { version, .. } | PackageIdentifier::Name { version, .. } => {
                *version
            }
        }
    }

    /// Returns a random `PackageIdentifier`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let version = rng.gen::<bool>().then(|| rng.gen::<EntityVersion>());
        if rng.gen() {
            PackageIdentifier::Hash {
                package_hash: PackageHash::new(rng.gen()),
                version,
            }
        } else {
            PackageIdentifier::Name {
                name: rng.random_string(1..21),
                version,
            }
        }
    }
}

impl Display for PackageIdentifier {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            PackageIdentifier::Hash {
                package_hash: contract_package_hash,
                version: Some(ver),
            } => write!(
                formatter,
                "package-id({}, version {})",
                HexFmt(contract_package_hash),
                ver
            ),
            PackageIdentifier::Hash {
                package_hash: contract_package_hash,
                ..
            } => write!(
                formatter,
                "package-id({}, latest)",
                HexFmt(contract_package_hash),
            ),
            PackageIdentifier::Name {
                name,
                version: Some(ver),
            } => write!(formatter, "package-id({}, version {})", name, ver),
            PackageIdentifier::Name { name, .. } => {
                write!(formatter, "package-id({}, latest)", name)
            }
        }
    }
}

impl ToBytes for PackageIdentifier {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PackageIdentifier::Hash {
                package_hash,
                version,
            } => {
                HASH_TAG.write_bytes(writer)?;
                package_hash.write_bytes(writer)?;
                version.write_bytes(writer)
            }
            PackageIdentifier::Name { name, version } => {
                NAME_TAG.write_bytes(writer)?;
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
                PackageIdentifier::Hash {
                    package_hash,
                    version,
                } => package_hash.serialized_length() + version.serialized_length(),
                PackageIdentifier::Name { name, version } => {
                    name.serialized_length() + version.serialized_length()
                }
            }
    }
}

impl FromBytes for PackageIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            HASH_TAG => {
                let (package_hash, remainder) = PackageHash::from_bytes(remainder)?;
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let id = PackageIdentifier::Hash {
                    package_hash,
                    version,
                };
                Ok((id, remainder))
            }
            NAME_TAG => {
                let (name, remainder) = String::from_bytes(remainder)?;
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let id = PackageIdentifier::Name { name, version };
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
        bytesrepr::test_serialization_roundtrip(&PackageIdentifier::random(rng));
    }
}
