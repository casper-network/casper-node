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
    contracts::ProtocolVersionMajor,
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
        /// The major protocol version of the contract package.
        ///
        /// `None` implies latest major protocol version.
        #[serde(skip_serializing_if = "Option::is_none")]
        protocol_version_major: Option<ProtocolVersionMajor>,
    },
    /// The alias and optional version identifying the package.
    ByPackageName {
        /// The package name.
        name: String,
        /// The package version.
        ///
        /// If `None`, the latest enabled version is implied.
        version: Option<EntityVersion>,
        /// The major protocol version of the contract package.
        ///
        /// `None` implies latest major protocol version.
        #[serde(skip_serializing_if = "Option::is_none")]
        protocol_version_major: Option<ProtocolVersionMajor>,
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
    #[deprecated(since = "5.0.1", note = "please use `new_package_with_major` instead")]
    pub fn new_package(hash: PackageHash, version: Option<EntityVersion>) -> Self {
        TransactionInvocationTarget::ByPackageHash {
            addr: hash.value(),
            version,
            protocol_version_major: None,
        }
    }

    /// Returns a new `TransactionInvocationTarget::Package`.
    pub fn new_package_with_major(
        hash: PackageHash,
        version: Option<EntityVersion>,
        protocol_version_major: Option<ProtocolVersionMajor>,
    ) -> Self {
        TransactionInvocationTarget::ByPackageHash {
            addr: hash.value(),
            version,
            protocol_version_major,
        }
    }

    /// Returns a new `TransactionInvocationTarget::PackageAlias`.
    #[deprecated(
        since = "5.0.1",
        note = "please use `new_package_alias_with_major` instead"
    )]
    pub fn new_package_alias(alias: String, version: Option<EntityVersion>) -> Self {
        TransactionInvocationTarget::ByPackageName {
            name: alias,
            version,
            protocol_version_major: None,
        }
    }

    /// Returns a new `TransactionInvocationTarget::PackageAlias`.
    pub fn new_package_alias_with_major(
        alias: String,
        version: Option<EntityVersion>,
        protocol_version_major: Option<ProtocolVersionMajor>,
    ) -> Self {
        TransactionInvocationTarget::ByPackageName {
            name: alias,
            version,
            protocol_version_major,
        }
    }

    #[cfg(test)]
    pub fn new_package_alias_with_major_and_entity(
        hash: PackageHash,
        version: Option<EntityVersion>,
        protocol_version_major: Option<ProtocolVersionMajor>,
    ) -> Self {
        TransactionInvocationTarget::ByPackageHash {
            addr: hash.value(),
            version,
            protocol_version_major,
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
            TransactionInvocationTarget::ByPackageHash {
                addr,
                version,
                protocol_version_major,
            } => Some(PackageIdentifier::HashWithMajorVersion {
                package_hash: PackageHash::new(*addr),
                version: *version,
                protocol_version_major: *protocol_version_major,
            }),
            TransactionInvocationTarget::ByPackageName {
                name: alias,
                version,
                protocol_version_major,
            } => Some(PackageIdentifier::NameWithMajorVersion {
                name: alias.clone(),
                version: *version,
                protocol_version_major: *protocol_version_major,
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
            TransactionInvocationTarget::ByPackageHash {
                addr,
                version,
                protocol_version_major,
            } => {
                let mut field_sizes = vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    addr.serialized_length(),
                    version.serialized_length(),
                ];
                if let Some(protocol_version_major) = protocol_version_major {
                    //When we serialize protocol_version_major we put the actual value,
                    // if we want to denote `None` we don't put an entry in the calltable.
                    field_sizes.push(protocol_version_major.serialized_length());
                }
                field_sizes
            }
            TransactionInvocationTarget::ByPackageName {
                name,
                version,
                protocol_version_major,
            } => {
                let mut field_sizes = vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    name.serialized_length(),
                    version.serialized_length(),
                ];
                if let Some(protocol_version_major) = protocol_version_major {
                    //When we serialize protocol_version_major we put the actual value,
                    // if we want to denote `None` we don't put an entry in the calltable.
                    field_sizes.push(protocol_version_major.serialized_length());
                }
                field_sizes
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
                version: rng.gen(),
                protocol_version_major: rng.gen(),
            },
            3 => TransactionInvocationTarget::ByPackageName {
                name: rng.random_string(1..21),
                version: rng.gen(),
                protocol_version_major: rng.gen(),
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
const BY_PACKAGE_HASH_PROTOCOL_VERSION_MAJOR_INDEX: u16 = 3;

const BY_PACKAGE_NAME_VARIANT: u8 = 3;
const BY_PACKAGE_NAME_NAME_INDEX: u16 = 1;
const BY_PACKAGE_NAME_VERSION_INDEX: u16 = 2;
const BY_PACKAGE_NAME_PROTOCOL_VERSION_MAJOR_INDEX: u16 = 3;

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
            TransactionInvocationTarget::ByPackageHash {
                addr,
                version,
                protocol_version_major,
            } => {
                let mut builder =
                    CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                        .add_field(TAG_FIELD_INDEX, &BY_PACKAGE_HASH_VARIANT)?
                        .add_field(BY_PACKAGE_HASH_ADDR_INDEX, &addr)?
                        .add_field(BY_PACKAGE_HASH_VERSION_INDEX, &version)?;
                if let Some(protocol_version_major) = protocol_version_major {
                    //We do this to support transactions that were created before the
                    // `protocol_version_major` fix. The pre-fix transactions
                    // will not have a BY_PACKAGE_HASH_PROTOCOL_VERSION_MAJOR_INDEX
                    // entry and we need to maintain ability to deserialize them.
                    builder = builder.add_field(
                        BY_PACKAGE_HASH_PROTOCOL_VERSION_MAJOR_INDEX,
                        protocol_version_major,
                    )?;
                }
                builder.binary_payload_bytes()
            }
            TransactionInvocationTarget::ByPackageName {
                name,
                version,
                protocol_version_major,
            } => {
                let mut builder =
                    CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                        .add_field(TAG_FIELD_INDEX, &BY_PACKAGE_NAME_VARIANT)?
                        .add_field(BY_PACKAGE_NAME_NAME_INDEX, &name)?
                        .add_field(BY_PACKAGE_NAME_VERSION_INDEX, &version)?;
                if let Some(protocol_version_major) = protocol_version_major {
                    //We do this to support transactions that were created before the
                    // `protocol_version_major` fix. The pre-fix transactions
                    // will not have a BY_PACKAGE_HASH_PROTOCOL_VERSION_MAJOR_INDEX
                    // entry and we need to maintain ability to deserialize them.
                    builder = builder.add_field(
                        BY_PACKAGE_NAME_PROTOCOL_VERSION_MAJOR_INDEX,
                        protocol_version_major,
                    )?;
                }
                builder.binary_payload_bytes()
            }
        }
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for TransactionInvocationTarget {
    fn from_bytes(bytes: &[u8]) -> Result<(TransactionInvocationTarget, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(4, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;
        window.verify_index(TAG_FIELD_INDEX)?;
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            BY_HASH_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(BY_HASH_HASH_INDEX)?;
                let (hash, window) = window.deserialize_and_maybe_next::<HashAddr>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionInvocationTarget::ByHash(hash))
            }
            BY_NAME_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(BY_NAME_NAME_INDEX)?;
                let (name, window) = window.deserialize_and_maybe_next::<String>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionInvocationTarget::ByName(name))
            }
            BY_PACKAGE_HASH_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(BY_PACKAGE_HASH_ADDR_INDEX)?;
                let (addr, window) = window.deserialize_and_maybe_next::<PackageAddr>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(BY_PACKAGE_HASH_VERSION_INDEX)?;
                let (version, window) =
                    window.deserialize_and_maybe_next::<Option<EntityVersion>>()?;
                let protocol_version_major = if let Some(window) = window {
                    window.verify_index(BY_PACKAGE_HASH_PROTOCOL_VERSION_MAJOR_INDEX)?;
                    let (protocol_version_major, window) =
                        window.deserialize_and_maybe_next::<ProtocolVersionMajor>()?;
                    if window.is_some() {
                        return Err(Formatting);
                    }
                    Some(protocol_version_major)
                } else {
                    if window.is_some() {
                        return Err(Formatting);
                    }
                    None
                };

                Ok(TransactionInvocationTarget::ByPackageHash {
                    addr,
                    version,
                    protocol_version_major,
                })
            }
            BY_PACKAGE_NAME_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(BY_PACKAGE_NAME_NAME_INDEX)?;
                let (name, window) = window.deserialize_and_maybe_next::<String>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(BY_PACKAGE_NAME_VERSION_INDEX)?;
                let (version, window) =
                    window.deserialize_and_maybe_next::<Option<EntityVersion>>()?;
                let protocol_version_major = if let Some(window) = window {
                    window.verify_index(BY_PACKAGE_NAME_PROTOCOL_VERSION_MAJOR_INDEX)?;
                    let (protocol_version_major, window) =
                        window.deserialize_and_maybe_next::<ProtocolVersionMajor>()?;
                    if window.is_some() {
                        return Err(Formatting);
                    }
                    Some(protocol_version_major)
                } else {
                    if window.is_some() {
                        return Err(Formatting);
                    }
                    None
                };
                Ok(TransactionInvocationTarget::ByPackageName {
                    name,
                    version,
                    protocol_version_major,
                })
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
                version,
                protocol_version_major,
            } => {
                write!(
                    formatter,
                    "package({:10}, version {:?}, protocol_version_major {:?})",
                    HexFmt(addr),
                    version,
                    protocol_version_major
                )
            }
            TransactionInvocationTarget::ByPackageName {
                name: alias,
                version,
                protocol_version_major,
            } => {
                write!(
                    formatter,
                    "package({}, version {:?}, protocol_version_major {:?})",
                    alias, version, protocol_version_major
                )
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
            TransactionInvocationTarget::ByPackageHash {
                addr,
                version,
                protocol_version_major,
            } => formatter
                .debug_struct("Package")
                .field("addr", &HexFmt(addr))
                .field("version", version)
                .field("protocol_version_major", protocol_version_major)
                .finish(),
            TransactionInvocationTarget::ByPackageName {
                name: alias,
                version,
                protocol_version_major,
            } => formatter
                .debug_struct("PackageAlias")
                .field("alias", alias)
                .field("version", version)
                .field("protocol_version_major", protocol_version_major)
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
    fn json_should_not_produce_version_key_if_none() {
        let alias = TransactionInvocationTarget::new_package_alias_with_major(
            "abc".to_owned(),
            Some(111),
            None,
        );
        assert!(!serde_json::to_string(&alias)
            .unwrap()
            .contains("\"protocol_version_major\""));

        let alias = TransactionInvocationTarget::new_package_alias_with_major(
            "abc".to_owned(),
            Some(111),
            Some(5),
        );
        assert!(serde_json::to_string(&alias)
            .unwrap()
            .contains("\"protocol_version_major\":5"));

        let package = TransactionInvocationTarget::new_package_with_major(
            PackageHash::from([1; 32]),
            Some(222),
            None,
        );
        assert!(!serde_json::to_string(&package)
            .unwrap()
            .contains("\"protocol_version_major\""));

        let package = TransactionInvocationTarget::new_package_with_major(
            PackageHash::from([1; 32]),
            Some(222),
            Some(5),
        );
        assert!(serde_json::to_string(&package)
            .unwrap()
            .contains("\"protocol_version_major\":5"));
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&TransactionInvocationTarget::random(rng));
        }
    }

    #[test]
    fn by_package_hash_variant_without_version_key_should_serialize_exactly_as_before_the_version_key_change(
    ) {
        let addr = [1; 32];
        let version = Some(1200);
        let field_sizes = vec![
            crate::bytesrepr::U8_SERIALIZED_LENGTH,
            addr.serialized_length(),
            version.serialized_length(),
        ];
        let builder = CalltableSerializationEnvelopeBuilder::new(field_sizes)
            .unwrap()
            .add_field(TAG_FIELD_INDEX, &BY_PACKAGE_HASH_VARIANT)
            .unwrap()
            .add_field(BY_PACKAGE_HASH_ADDR_INDEX, &addr)
            .unwrap()
            .add_field(BY_PACKAGE_HASH_VERSION_INDEX, &version)
            .unwrap();
        let bytes = builder.binary_payload_bytes().unwrap();
        let expected = TransactionInvocationTarget::ByPackageHash {
            addr,
            version,
            protocol_version_major: None,
        };
        let expected_bytes = expected.to_bytes().unwrap();
        assert_eq!(bytes, expected_bytes); //We want the "legacy" binary representation and current representation without
                                           // protocol_version_major equal

        let (got, remainder) = TransactionInvocationTarget::from_bytes(&bytes).unwrap();
        assert_eq!(expected, got);
        assert!(remainder.is_empty());
    }

    #[test]
    fn by_package_name_variant_without_version_key_should_serialize_exactly_as_before_the_version_key_change(
    ) {
        let name = "some_name".to_string();
        let version = Some(1200);
        let field_sizes = vec![
            crate::bytesrepr::U8_SERIALIZED_LENGTH,
            name.serialized_length(),
            version.serialized_length(),
        ];
        let builder = CalltableSerializationEnvelopeBuilder::new(field_sizes)
            .unwrap()
            .add_field(TAG_FIELD_INDEX, &BY_PACKAGE_NAME_VARIANT)
            .unwrap()
            .add_field(BY_PACKAGE_NAME_NAME_INDEX, &name)
            .unwrap()
            .add_field(BY_PACKAGE_NAME_VERSION_INDEX, &version)
            .unwrap();
        let bytes = builder.binary_payload_bytes().unwrap();
        let expected = TransactionInvocationTarget::ByPackageName {
            name,
            version,
            protocol_version_major: None,
        };
        let expected_bytes = expected.to_bytes().unwrap();
        assert_eq!(bytes, expected_bytes); //We want the "legacy" binary representation and current representation without
                                           // protocol_version_major equal

        let (got, remainder) = TransactionInvocationTarget::from_bytes(&bytes).unwrap();
        assert_eq!(expected, got);
        assert!(remainder.is_empty());
    }

    #[test]
    fn by_package_hash_variant_should_deserialize_bytes_that_have_both_version_and_key() {
        let target = TransactionInvocationTarget::ByPackageHash {
            addr: [1; 32],
            version: Some(11),
            protocol_version_major: Some(2),
        };
        let bytes = target.to_bytes().unwrap();
        let (number_of_fields, _) = u32::from_bytes(&bytes).unwrap();
        assert_eq!(number_of_fields, 4); //We want the enum tag, addr, version (even if it's None) and protocol_version_major to
                                         // have been serialized
        let (got, remainder) = TransactionInvocationTarget::from_bytes(&bytes).unwrap();
        assert_eq!(target, got);
        assert!(remainder.is_empty());
    }

    #[test]
    fn by_package_name_variant_should_deserialize_bytes_that_have_both_version_and_key() {
        let target = TransactionInvocationTarget::ByPackageName {
            name: "xyz".to_string(),
            version: Some(11),
            protocol_version_major: Some(3),
        };
        let bytes = target.to_bytes().unwrap();
        let (number_of_fields, _) = u32::from_bytes(&bytes).unwrap();
        assert_eq!(number_of_fields, 4); //We want the enum tag, addr, version (even if it's None) and protocol_version_major to
                                         // have been serialized
        let (got, remainder) = TransactionInvocationTarget::from_bytes(&bytes).unwrap();
        assert_eq!(target, got);
        assert!(remainder.is_empty());
    }

    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in transaction_invocation_target_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
