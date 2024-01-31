use alloc::{string::String, vec::Vec};
use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use hex_fmt::HexFmt;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::AddressableEntityIdentifier;
#[cfg(doc)]
use super::TransactionTarget;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    serde_helpers, AddressableEntityHash, EntityAddr, EntityVersion, PackageAddr, PackageHash,
    PackageIdentifier,
};

const INVOCABLE_ENTITY_TAG: u8 = 0;
const INVOCABLE_ENTITY_ALIAS_TAG: u8 = 1;
const PACKAGE_TAG: u8 = 2;
const PACKAGE_ALIAS_TAG: u8 = 3;

/// The identifier of a [`TransactionTarget::Stored`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Identifier of a `Stored` transaction target.")
)]
#[serde(deny_unknown_fields)]
pub enum TransactionInvocationTarget {
    /// The address identifying the invocable entity.
    #[serde(with = "serde_helpers::raw_32_byte_array")]
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            with = "String",
            description = "Hex-encoded entity address identifying the invocable entity."
        )
    )]
    InvocableEntity(EntityAddr), // currently needs to be of contract tag variant
    /// The alias identifying the invocable entity.
    InvocableEntityAlias(String),
    /// The address and optional version identifying the package.
    Package {
        /// The package address.
        #[serde(with = "serde_helpers::raw_32_byte_array")]
        #[cfg_attr(
            feature = "json-schema",
            schemars(with = "String", description = "Hex-encoded address of the package.")
        )]
        addr: PackageAddr,
        /// The package version.
        ///
        /// If `None`, the latest enabled version is implied.
        version: Option<EntityVersion>,
    },
    /// The alias and optional version identifying the package.
    PackageAlias {
        /// The package alias.
        alias: String,
        /// The package version.
        ///
        /// If `None`, the latest enabled version is implied.
        version: Option<EntityVersion>,
    },
}

impl TransactionInvocationTarget {
    /// Returns a new `TransactionInvocationTarget::InvocableEntity`.
    pub fn new_invocable_entity(addr: EntityAddr) -> Self {
        TransactionInvocationTarget::InvocableEntity(addr)
    }

    /// Returns a new `TransactionInvocationTarget::InvocableEntityAlias`.
    pub fn new_invocable_entity_alias(alias: String) -> Self {
        TransactionInvocationTarget::InvocableEntityAlias(alias)
    }

    /// Returns a new `TransactionInvocationTarget::Package`.
    pub fn new_package(addr: PackageAddr, version: Option<EntityVersion>) -> Self {
        TransactionInvocationTarget::Package { addr, version }
    }

    /// Returns a new `TransactionInvocationTarget::PackageAlias`.
    pub fn new_package_alias(alias: String, version: Option<EntityVersion>) -> Self {
        TransactionInvocationTarget::PackageAlias { alias, version }
    }

    /// Returns the identifier of the addressable entity, if present.
    pub fn addressable_entity_identifier(&self) -> Option<AddressableEntityIdentifier> {
        match self {
            TransactionInvocationTarget::InvocableEntity(addr) => Some(
                AddressableEntityIdentifier::Hash(AddressableEntityHash::new(*addr)),
            ),
            TransactionInvocationTarget::InvocableEntityAlias(alias) => {
                Some(AddressableEntityIdentifier::Name(alias.clone()))
            }
            TransactionInvocationTarget::Package { .. }
            | TransactionInvocationTarget::PackageAlias { .. } => None,
        }
    }

    /// Returns the identifier of the contract package, if present.
    pub fn package_identifier(&self) -> Option<PackageIdentifier> {
        match self {
            TransactionInvocationTarget::InvocableEntity(_)
            | TransactionInvocationTarget::InvocableEntityAlias(_) => None,
            TransactionInvocationTarget::Package { addr, version } => {
                Some(PackageIdentifier::Hash {
                    package_hash: PackageHash::new(*addr),
                    version: *version,
                })
            }
            TransactionInvocationTarget::PackageAlias { alias, version } => {
                Some(PackageIdentifier::Name {
                    name: alias.clone(),
                    version: *version,
                })
            }
        }
    }

    /// Returns a random `TransactionInvocationTarget`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..4) {
            INVOCABLE_ENTITY_TAG => TransactionInvocationTarget::InvocableEntity(rng.gen()),
            INVOCABLE_ENTITY_ALIAS_TAG => {
                TransactionInvocationTarget::InvocableEntityAlias(rng.random_string(1..21))
            }
            PACKAGE_TAG => TransactionInvocationTarget::Package {
                addr: rng.gen(),
                version: rng.gen::<bool>().then(|| rng.gen::<EntityVersion>()),
            },
            PACKAGE_ALIAS_TAG => TransactionInvocationTarget::PackageAlias {
                alias: rng.random_string(1..21),
                version: rng.gen::<bool>().then(|| rng.gen::<EntityVersion>()),
            },
            _ => unreachable!(),
        }
    }
}

impl Display for TransactionInvocationTarget {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionInvocationTarget::InvocableEntity(addr) => {
                write!(formatter, "invocable-entity({:10})", HexFmt(addr))
            }
            TransactionInvocationTarget::InvocableEntityAlias(alias) => {
                write!(formatter, "invocable-entity({})", alias)
            }
            TransactionInvocationTarget::Package {
                addr,
                version: Some(ver),
            } => {
                write!(formatter, "package({:10}, version {})", HexFmt(addr), ver)
            }
            TransactionInvocationTarget::Package {
                addr,
                version: None,
            } => {
                write!(formatter, "package({:10}, latest)", HexFmt(addr))
            }
            TransactionInvocationTarget::PackageAlias {
                alias,
                version: Some(ver),
            } => {
                write!(formatter, "package({}, version {})", alias, ver)
            }
            TransactionInvocationTarget::PackageAlias {
                alias,
                version: None,
            } => {
                write!(formatter, "package({}, latest)", alias)
            }
        }
    }
}

impl Debug for TransactionInvocationTarget {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionInvocationTarget::InvocableEntity(addr) => formatter
                .debug_tuple("InvocableEntity")
                .field(&HexFmt(addr))
                .finish(),
            TransactionInvocationTarget::InvocableEntityAlias(alias) => formatter
                .debug_tuple("InvocableEntityAlias")
                .field(alias)
                .finish(),
            TransactionInvocationTarget::Package { addr, version } => formatter
                .debug_struct("Package")
                .field("addr", &HexFmt(addr))
                .field("version", version)
                .finish(),
            TransactionInvocationTarget::PackageAlias { alias, version } => formatter
                .debug_struct("PackageAlias")
                .field("alias", alias)
                .field("version", version)
                .finish(),
        }
    }
}

impl ToBytes for TransactionInvocationTarget {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionInvocationTarget::InvocableEntity(addr) => {
                INVOCABLE_ENTITY_TAG.write_bytes(writer)?;
                addr.write_bytes(writer)
            }
            TransactionInvocationTarget::InvocableEntityAlias(alias) => {
                INVOCABLE_ENTITY_ALIAS_TAG.write_bytes(writer)?;
                alias.write_bytes(writer)
            }
            TransactionInvocationTarget::Package { addr, version } => {
                PACKAGE_TAG.write_bytes(writer)?;
                addr.write_bytes(writer)?;
                version.write_bytes(writer)
            }
            TransactionInvocationTarget::PackageAlias { alias, version } => {
                PACKAGE_ALIAS_TAG.write_bytes(writer)?;
                alias.write_bytes(writer)?;
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
                TransactionInvocationTarget::InvocableEntity(addr) => addr.serialized_length(),
                TransactionInvocationTarget::InvocableEntityAlias(alias) => {
                    alias.serialized_length()
                }
                TransactionInvocationTarget::Package { addr, version } => {
                    addr.serialized_length() + version.serialized_length()
                }
                TransactionInvocationTarget::PackageAlias { alias, version } => {
                    alias.serialized_length() + version.serialized_length()
                }
            }
    }
}

impl FromBytes for TransactionInvocationTarget {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            INVOCABLE_ENTITY_TAG => {
                let (addr, remainder) = EntityAddr::from_bytes(remainder)?;
                let target = TransactionInvocationTarget::InvocableEntity(addr);
                Ok((target, remainder))
            }
            INVOCABLE_ENTITY_ALIAS_TAG => {
                let (alias, remainder) = String::from_bytes(remainder)?;
                let target = TransactionInvocationTarget::InvocableEntityAlias(alias);
                Ok((target, remainder))
            }
            PACKAGE_TAG => {
                let (addr, remainder) = PackageAddr::from_bytes(remainder)?;
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let target = TransactionInvocationTarget::Package { addr, version };
                Ok((target, remainder))
            }
            PACKAGE_ALIAS_TAG => {
                let (alias, remainder) = String::from_bytes(remainder)?;
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let target = TransactionInvocationTarget::PackageAlias { alias, version };
                Ok((target, remainder))
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
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&TransactionInvocationTarget::random(rng));
        }
    }
}
