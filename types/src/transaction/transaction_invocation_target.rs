use alloc::{string::String, vec::Vec};
use core::fmt::{self, Debug, Display, Formatter};

use super::{serialization::CalltableSerializationEnvelope, AddressableEntityIdentifier};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    serde_helpers,
    transaction::serialization::CalltableSerializationEnvelopeBuilder,
    AddressableEntityHash, EntityVersion, HashAddr, PackageAddr, PackageHash, PackageIdentifier,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use hex_fmt::HexFmt;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The identifier of a [`crate::TransactionTarget::Stored`].
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
    ByHash(HashAddr), /* currently needs to be of contract tag
                       * variant */
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

    /// Returns the contract `hash_addr`, if any.
    pub fn contract_by_hash(&self) -> Option<HashAddr> {
        if let TransactionInvocationTarget::ByHash(hash_addr) = self {
            Some(*hash_addr)
        } else {
            None
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

    fn serialized_field_lengths(&self) -> Vec<usize> {
        match self {
            TransactionInvocationTarget::ByHash(hash) => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    hash.serialized_length(),
                ]
            }
            TransactionInvocationTarget::ByName(name) => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    name.serialized_length(),
                ]
            }
            TransactionInvocationTarget::ByPackageHash { addr, version } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    addr.serialized_length(),
                    version.serialized_length(),
                ]
            }
            TransactionInvocationTarget::ByPackageName { name, version } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    name.serialized_length(),
                    version.serialized_length(),
                ]
            }
        }
    }

    /// Returns a random `TransactionInvocationTarget`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..4) {
            0 => TransactionInvocationTarget::ByHash(rng.gen()),
            1 => TransactionInvocationTarget::ByName(rng.random_string(1..21)),
            2 => TransactionInvocationTarget::ByPackageHash {
                addr: rng.gen(),
                version: rng.gen::<bool>().then(|| rng.gen::<EntityVersion>()),
            },
            3 => TransactionInvocationTarget::ByPackageName {
                name: rng.random_string(1..21),
                version: rng.gen::<bool>().then(|| rng.gen::<EntityVersion>()),
            },
            _ => unreachable!(),
        }
    }
}

const TAG_FIELD_INDEX: u16 = 0;

const BY_HASH_VARIANT: u8 = 0;
const BY_HASH_HASH_INDEX: u16 = 1;

const BY_NAME_VARIANT: u8 = 1;
const BY_NAME_NAME_INDEX: u16 = 1;

const BY_PACKAGE_HASH_VARIANT: u8 = 2;
const BY_PACKAGE_HASH_ADDR_INDEX: u16 = 1;
const BY_PACKAGE_HASH_VERSION_INDEX: u16 = 2;

const BY_PACKAGE_NAME_VARIANT: u8 = 3;
const BY_PACKAGE_NAME_NAME_INDEX: u16 = 1;
const BY_PACKAGE_NAME_VERSION_INDEX: u16 = 2;

impl ToBytes for TransactionInvocationTarget {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            TransactionInvocationTarget::ByHash(hash) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &BY_HASH_VARIANT)?
                    .add_field(BY_HASH_HASH_INDEX, &hash)?
                    .binary_payload_bytes()
            }
            TransactionInvocationTarget::ByName(name) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &BY_NAME_VARIANT)?
                    .add_field(BY_NAME_NAME_INDEX, &name)?
                    .binary_payload_bytes()
            }
            TransactionInvocationTarget::ByPackageHash { addr, version } => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &BY_PACKAGE_HASH_VARIANT)?
                    .add_field(BY_PACKAGE_HASH_ADDR_INDEX, &addr)?
                    .add_field(BY_PACKAGE_HASH_VERSION_INDEX, &version)?
                    .binary_payload_bytes()
            }
            TransactionInvocationTarget::ByPackageName { name, version } => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &BY_PACKAGE_NAME_VARIANT)?
                    .add_field(BY_PACKAGE_NAME_NAME_INDEX, &name)?
                    .add_field(BY_PACKAGE_NAME_VERSION_INDEX, &version)?
                    .binary_payload_bytes()
            }
        }
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for TransactionInvocationTarget {
    fn from_bytes(bytes: &[u8]) -> Result<(TransactionInvocationTarget, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(3, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;
        window.verify_index(0)?;
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            BY_HASH_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(1u16)?;
                let (hash, window) = window.deserialize_and_maybe_next::<HashAddr>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionInvocationTarget::ByHash(hash))
            }
            BY_NAME_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(1u16)?;
                let (name, window) = window.deserialize_and_maybe_next::<String>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionInvocationTarget::ByName(name))
            }
            BY_PACKAGE_HASH_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(1u16)?;
                let (addr, window) = window.deserialize_and_maybe_next::<PackageAddr>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(2u16)?;
                let (version, window) =
                    window.deserialize_and_maybe_next::<Option<EntityVersion>>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionInvocationTarget::ByPackageHash { addr, version })
            }
            BY_PACKAGE_NAME_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(1u16)?;
                let (name, window) = window.deserialize_and_maybe_next::<String>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(2u16)?;
                let (version, window) =
                    window.deserialize_and_maybe_next::<Option<EntityVersion>>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionInvocationTarget::ByPackageName { name, version })
            }
            _ => Err(Formatting),
        };
        to_ret.map(|endpoint| (endpoint, remainder))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr, gens::transaction_invocation_target_arb};
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
