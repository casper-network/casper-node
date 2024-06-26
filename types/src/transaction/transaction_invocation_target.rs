use alloc::{string::String, vec::Vec};
use core::fmt::{self, Debug, Display, Formatter};

use super::{serialization::transaction_invocation_target::*, AddressableEntityIdentifier};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    serde_helpers, AddressableEntityHash, EntityVersion, HashAddr, PackageAddr, PackageHash,
    PackageIdentifier,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use hex_fmt::HexFmt;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    ByHash(HashAddr), // currently needs to be of contract tag variant
    /// The alias identifying the invocable entity.
    ByName(String),
    /// The address and optional version identifying the package.
    ByPackageHash {
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
    ByPackageName {
        /// The package name.
        name: String,
        /// The package version.
        ///
        /// If `None`, the latest enabled version is implied.
        version: Option<EntityVersion>,
    },
}

impl TransactionInvocationTarget {
    /// Returns a new `TransactionInvocationTarget::InvocableEntity`.
    pub fn new_invocable_entity(hash: AddressableEntityHash) -> Self {
        TransactionInvocationTarget::ByHash(hash.value())
    }

    /// Returns a new `TransactionInvocationTarget::InvocableEntityAlias`.
    pub fn new_invocable_entity_alias(alias: String) -> Self {
        TransactionInvocationTarget::ByName(alias)
    }

    /// Returns a new `TransactionInvocationTarget::Package`.
    pub fn new_package(hash: PackageHash, version: Option<EntityVersion>) -> Self {
        TransactionInvocationTarget::ByPackageHash {
            addr: hash.value(),
            version,
        }
    }

    /// Returns a new `TransactionInvocationTarget::PackageAlias`.
    pub fn new_package_alias(alias: String, version: Option<EntityVersion>) -> Self {
        TransactionInvocationTarget::ByPackageName {
            name: alias,
            version,
        }
    }

    /// Returns the identifier of the addressable entity, if present.
    pub fn addressable_entity_identifier(&self) -> Option<AddressableEntityIdentifier> {
        match self {
            TransactionInvocationTarget::ByHash(addr) => Some(AddressableEntityIdentifier::Hash(
                AddressableEntityHash::new(*addr),
            )),
            TransactionInvocationTarget::ByName(alias) => {
                Some(AddressableEntityIdentifier::Name(alias.clone()))
            }
            TransactionInvocationTarget::ByPackageHash { .. }
            | TransactionInvocationTarget::ByPackageName { .. } => None,
        }
    }

    /// Returns the identifier of the contract package, if present.
    pub fn package_identifier(&self) -> Option<PackageIdentifier> {
        match self {
            TransactionInvocationTarget::ByHash(_) | TransactionInvocationTarget::ByName(_) => None,
            TransactionInvocationTarget::ByPackageHash { addr, version } => {
                Some(PackageIdentifier::Hash {
                    package_hash: PackageHash::new(*addr),
                    version: *version,
                })
            }
            TransactionInvocationTarget::ByPackageName {
                name: alias,
                version,
            } => Some(PackageIdentifier::Name {
                name: alias.clone(),
                version: *version,
            }),
        }
    }

    /// Returns a random `TransactionInvocationTarget`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..4) {
            INVOCABLE_ENTITY_TAG => TransactionInvocationTarget::ByHash(rng.gen()),
            INVOCABLE_ENTITY_ALIAS_TAG => {
                TransactionInvocationTarget::ByName(rng.random_string(1..21))
            }
            PACKAGE_TAG => TransactionInvocationTarget::ByPackageHash {
                addr: rng.gen(),
                version: rng.gen::<bool>().then(|| rng.gen::<EntityVersion>()),
            },
            PACKAGE_ALIAS_TAG => TransactionInvocationTarget::ByPackageName {
                name: rng.random_string(1..21),
                version: rng.gen::<bool>().then(|| rng.gen::<EntityVersion>()),
            },
            _ => unreachable!(),
        }
    }
}

impl Display for TransactionInvocationTarget {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionInvocationTarget::ByHash(addr) => {
                write!(formatter, "invocable-entity({:10})", HexFmt(addr))
            }
            TransactionInvocationTarget::ByName(alias) => {
                write!(formatter, "invocable-entity({})", alias)
            }
            TransactionInvocationTarget::ByPackageHash {
                addr,
                version: Some(ver),
            } => {
                write!(formatter, "package({:10}, version {})", HexFmt(addr), ver)
            }
            TransactionInvocationTarget::ByPackageHash {
                addr,
                version: None,
            } => {
                write!(formatter, "package({:10}, latest)", HexFmt(addr))
            }
            TransactionInvocationTarget::ByPackageName {
                name: alias,
                version: Some(ver),
            } => {
                write!(formatter, "package({}, version {})", alias, ver)
            }
            TransactionInvocationTarget::ByPackageName {
                name: alias,
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
            TransactionInvocationTarget::ByHash(addr) => formatter
                .debug_tuple("InvocableEntity")
                .field(&HexFmt(addr))
                .finish(),
            TransactionInvocationTarget::ByName(alias) => formatter
                .debug_tuple("InvocableEntityAlias")
                .field(alias)
                .finish(),
            TransactionInvocationTarget::ByPackageHash { addr, version } => formatter
                .debug_struct("Package")
                .field("addr", &HexFmt(addr))
                .field("version", version)
                .finish(),
            TransactionInvocationTarget::ByPackageName {
                name: alias,
                version,
            } => formatter
                .debug_struct("PackageAlias")
                .field("alias", alias)
                .field("version", version)
                .finish(),
        }
    }
}

impl ToBytes for TransactionInvocationTarget {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            TransactionInvocationTarget::ByHash(addr) => serialize_by_hash(addr),
            TransactionInvocationTarget::ByName(alias) => serialize_by_name(alias),
            TransactionInvocationTarget::ByPackageHash { addr, version } => {
                serialize_by_package_hash(addr, version)
            }
            TransactionInvocationTarget::ByPackageName { name, version } => {
                serialize_by_package_name(name, version)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            TransactionInvocationTarget::ByHash(addr) => by_hash_serialized_length(addr),
            TransactionInvocationTarget::ByName(alias) => by_name_serialized_length(alias),
            TransactionInvocationTarget::ByPackageHash { addr, version } => {
                by_package_hash_serialized_length(addr, version)
            }
            TransactionInvocationTarget::ByPackageName { name, version } => {
                by_package_name_serialized_length(name, version)
            }
        }
    }
}

impl FromBytes for TransactionInvocationTarget {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        deserialize_transaction_invocation_target(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gens::transaction_invocation_target_arb;
    use proptest::prelude::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&TransactionInvocationTarget::random(rng));
        }
    }
    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in transaction_invocation_target_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
