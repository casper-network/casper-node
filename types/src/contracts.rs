//! Data types for supporting contract headers feature.
// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    vec::Vec,
};
use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    account,
    addressable_entity::{NamedKeys, TryFromSliceForAccountHashError},
    bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH},
    checksummed_hex,
    contract_wasm::ContractWasmHash,
    package::PackageStatus,
    uref,
    uref::URef,
    AddressableEntityHash, CLType, CLTyped, EntityVersionKey, EntryPoint, EntryPoints, Groups,
    HashAddr, Key, Package, ProtocolVersion, KEY_HASH_LENGTH,
};

/// Maximum number of distinct user groups.
pub const MAX_GROUPS: u8 = 10;
/// Maximum number of URefs which can be assigned across all user groups.
pub const MAX_TOTAL_UREFS: usize = 100;

const CONTRACT_STRING_PREFIX: &str = "contract-";
const PACKAGE_STRING_PREFIX: &str = "contract-package-";
// We need to support the legacy prefix of "contract-package-wasm".
const PACKAGE_STRING_LEGACY_EXTRA_PREFIX: &str = "wasm";

/// Set of errors which may happen when working with contract headers.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
#[non_exhaustive]
pub enum Error {
    /// Attempt to override an existing or previously existing version with a
    /// new header (this is not allowed to ensure immutability of a given
    /// version).
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(1, Error::PreviouslyUsedVersion as u8);
    /// ```
    PreviouslyUsedVersion = 1,
    /// Attempted to disable a contract that does not exist.
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(2, Error::ContractNotFound as u8);
    /// ```
    ContractNotFound = 2,
    /// Attempted to create a user group which already exists (use the update
    /// function to change an existing user group).
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(3, Error::GroupAlreadyExists as u8);
    /// ```
    GroupAlreadyExists = 3,
    /// Attempted to add a new user group which exceeds the allowed maximum
    /// number of groups.
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(4, Error::MaxGroupsExceeded as u8);
    /// ```
    MaxGroupsExceeded = 4,
    /// Attempted to add a new URef to a group, which resulted in the total
    /// number of URefs across all user groups to exceed the allowed maximum.
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(5, Error::MaxTotalURefsExceeded as u8);
    /// ```
    MaxTotalURefsExceeded = 5,
    /// Attempted to remove a URef from a group, which does not exist in the
    /// group.
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(6, Error::GroupDoesNotExist as u8);
    /// ```
    GroupDoesNotExist = 6,
    /// Attempted to remove unknown URef from the group.
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(7, Error::UnableToRemoveURef as u8);
    /// ```
    UnableToRemoveURef = 7,
    /// Group is use by at least one active contract.
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(8, Error::GroupInUse as u8);
    /// ```
    GroupInUse = 8,
    /// URef already exists in given group.
    /// ```
    /// # use casper_types::contracts::Error;
    /// assert_eq!(9, Error::URefAlreadyExists as u8);
    /// ```
    URefAlreadyExists = 9,
}

impl TryFrom<u8> for Error {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let error = match value {
            v if v == Self::PreviouslyUsedVersion as u8 => Self::PreviouslyUsedVersion,
            v if v == Self::ContractNotFound as u8 => Self::ContractNotFound,
            v if v == Self::GroupAlreadyExists as u8 => Self::GroupAlreadyExists,
            v if v == Self::MaxGroupsExceeded as u8 => Self::MaxGroupsExceeded,
            v if v == Self::MaxTotalURefsExceeded as u8 => Self::MaxTotalURefsExceeded,
            v if v == Self::GroupDoesNotExist as u8 => Self::GroupDoesNotExist,
            v if v == Self::UnableToRemoveURef as u8 => Self::UnableToRemoveURef,
            v if v == Self::GroupInUse as u8 => Self::GroupInUse,
            v if v == Self::URefAlreadyExists as u8 => Self::URefAlreadyExists,
            _ => return Err(()),
        };
        Ok(error)
    }
}

/// Associated error type of `TryFrom<&[u8]>` for `ContractHash`.
#[derive(Debug)]
pub struct TryFromSliceForContractHashError(());

impl Display for TryFromSliceForContractHashError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "failed to retrieve from slice")
    }
}

/// An error from parsing a formatted contract string
#[derive(Debug)]
#[non_exhaustive]
pub enum FromStrError {
    /// Invalid formatted string prefix.
    InvalidPrefix,
    /// Error when decoding a hex string
    Hex(base16::DecodeError),
    /// Error when parsing an account
    Account(TryFromSliceForAccountHashError),
    /// Error when parsing the hash.
    Hash(TryFromSliceError),
    /// Error when parsing an account hash.
    AccountHash(account::FromStrError),
    /// Error when parsing an uref.
    URef(uref::FromStrError),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

impl From<TryFromSliceForAccountHashError> for FromStrError {
    fn from(error: TryFromSliceForAccountHashError) -> Self {
        FromStrError::Account(error)
    }
}

impl From<TryFromSliceError> for FromStrError {
    fn from(error: TryFromSliceError) -> Self {
        FromStrError::Hash(error)
    }
}

impl From<account::FromStrError> for FromStrError {
    fn from(error: account::FromStrError) -> Self {
        FromStrError::AccountHash(error)
    }
}

impl From<uref::FromStrError> for FromStrError {
    fn from(error: uref::FromStrError) -> Self {
        FromStrError::URef(error)
    }
}

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FromStrError::InvalidPrefix => write!(f, "invalid prefix"),
            FromStrError::Hex(error) => write!(f, "decode from hex: {}", error),
            FromStrError::Account(error) => write!(f, "account from string error: {:?}", error),
            FromStrError::Hash(error) => write!(f, "hash from string error: {}", error),
            FromStrError::AccountHash(error) => {
                write!(f, "account hash from string error: {:?}", error)
            }
            FromStrError::URef(error) => write!(f, "uref from string error: {:?}", error),
        }
    }
}

/// Automatically incremented value for a contract version within a major `ProtocolVersion`.
pub type ContractVersion = u32;

/// Within each discrete major `ProtocolVersion`, contract version resets to this value.
pub const CONTRACT_INITIAL_VERSION: ContractVersion = 1;

/// Major element of `ProtocolVersion` a `ContractVersion` is compatible with.
pub type ProtocolVersionMajor = u32;

/// Major element of `ProtocolVersion` combined with `ContractVersion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ContractVersionKey(ProtocolVersionMajor, ContractVersion);

impl ContractVersionKey {
    /// Returns a new instance of ContractVersionKey with provided values.
    pub fn new(
        protocol_version_major: ProtocolVersionMajor,
        contract_version: ContractVersion,
    ) -> Self {
        Self(protocol_version_major, contract_version)
    }

    /// Returns the major element of the protocol version this contract is compatible with.
    pub fn protocol_version_major(self) -> ProtocolVersionMajor {
        self.0
    }

    /// Returns the contract version within the protocol major version.
    pub fn contract_version(self) -> ContractVersion {
        self.1
    }
}

impl From<ContractVersionKey> for (ProtocolVersionMajor, ContractVersion) {
    fn from(contract_version_key: ContractVersionKey) -> Self {
        (contract_version_key.0, contract_version_key.1)
    }
}

/// Serialized length of `ContractVersionKey`.
pub const CONTRACT_VERSION_KEY_SERIALIZED_LENGTH: usize =
    U32_SERIALIZED_LENGTH + U32_SERIALIZED_LENGTH;

impl ToBytes for ContractVersionKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.0.to_bytes()?);
        ret.append(&mut self.1.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        CONTRACT_VERSION_KEY_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)?;
        self.1.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for ContractVersionKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (major, rem): (u32, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (contract, rem): (ContractVersion, &[u8]) = FromBytes::from_bytes(rem)?;
        Ok((ContractVersionKey::new(major, contract), rem))
    }
}

impl fmt::Display for ContractVersionKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.0, self.1)
    }
}

/// Collection of contract versions.
pub type ContractVersions = BTreeMap<ContractVersionKey, ContractHash>;

/// Collection of disabled contract versions. The runtime will not permit disabled
/// contract versions to be executed.
pub type DisabledVersions = BTreeSet<ContractVersionKey>;

/// A newtype wrapping a `HashAddr` which references a [`Contract`] in the global state.
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ContractHash(HashAddr);

impl ContractHash {
    /// Constructs a new `ContractHash` from the raw bytes of the contract hash.
    pub const fn new(value: HashAddr) -> ContractHash {
        ContractHash(value)
    }

    /// Returns the raw bytes of the contract hash as an array.
    pub fn value(&self) -> HashAddr {
        self.0
    }

    /// Returns the raw bytes of the contract hash as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `ContractHash` for users getting and putting.
    pub fn to_formatted_string(self) -> String {
        format!(
            "{}{}",
            CONTRACT_STRING_PREFIX,
            base16::encode_lower(&self.0),
        )
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a
    /// `ContractHash`.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(CONTRACT_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = HashAddr::try_from(checksummed_hex::decode(remainder)?.as_ref())?;
        Ok(ContractHash(bytes))
    }
}

impl Display for ContractHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for ContractHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "ContractHash({})", base16::encode_lower(&self.0))
    }
}

impl CLTyped for ContractHash {
    fn cl_type() -> CLType {
        CLType::ByteArray(KEY_HASH_LENGTH as u32)
    }
}

impl ToBytes for ContractHash {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    #[inline(always)]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.extend_from_slice(&self.0);
        Ok(())
    }
}

impl FromBytes for ContractHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((ContractHash::new(bytes), rem))
    }
}

impl From<[u8; 32]> for ContractHash {
    fn from(bytes: [u8; 32]) -> Self {
        ContractHash(bytes)
    }
}

impl Serialize for ContractHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for ContractHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            ContractHash::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = HashAddr::deserialize(deserializer)?;
            Ok(ContractHash(bytes))
        }
    }
}

impl AsRef<[u8]> for ContractHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TryFrom<&[u8]> for ContractHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForContractHashError> {
        HashAddr::try_from(bytes)
            .map(ContractHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

impl TryFrom<&Vec<u8>> for ContractHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        HashAddr::try_from(bytes as &[u8])
            .map(ContractHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

#[cfg(feature = "json-schema")]
impl JsonSchema for ContractHash {
    fn schema_name() -> String {
        String::from("ContractHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some("The hash address of the contract".to_string());
        schema_object.into()
    }
}

/// A newtype wrapping a `HashAddr` which references a [`ContractPackage`] in the global state.
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ContractPackageHash(HashAddr);

impl ContractPackageHash {
    /// Constructs a new `ContractPackageHash` from the raw bytes of the contract package hash.
    pub const fn new(value: HashAddr) -> ContractPackageHash {
        ContractPackageHash(value)
    }

    /// Returns the raw bytes of the contract hash as an array.
    pub fn value(&self) -> HashAddr {
        self.0
    }

    /// Returns the raw bytes of the contract hash as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `ContractPackageHash` for users getting and putting.
    pub fn to_formatted_string(self) -> String {
        format!("{}{}", PACKAGE_STRING_PREFIX, base16::encode_lower(&self.0),)
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a
    /// `ContractPackageHash`.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(PACKAGE_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;

        let hex_addr = remainder
            .strip_prefix(PACKAGE_STRING_LEGACY_EXTRA_PREFIX)
            .unwrap_or(remainder);

        let bytes = HashAddr::try_from(checksummed_hex::decode(hex_addr)?.as_ref())?;
        Ok(ContractPackageHash(bytes))
    }
}

impl Display for ContractPackageHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for ContractPackageHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "ContractPackageHash({})", base16::encode_lower(&self.0))
    }
}

impl CLTyped for ContractPackageHash {
    fn cl_type() -> CLType {
        CLType::ByteArray(KEY_HASH_LENGTH as u32)
    }
}

impl ToBytes for ContractPackageHash {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    #[inline(always)]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.extend_from_slice(&self.0);
        Ok(())
    }
}

impl FromBytes for ContractPackageHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((ContractPackageHash::new(bytes), rem))
    }
}

impl From<[u8; 32]> for ContractPackageHash {
    fn from(bytes: [u8; 32]) -> Self {
        ContractPackageHash(bytes)
    }
}

impl Serialize for ContractPackageHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for ContractPackageHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            ContractPackageHash::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = HashAddr::deserialize(deserializer)?;
            Ok(ContractPackageHash(bytes))
        }
    }
}

impl AsRef<[u8]> for ContractPackageHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TryFrom<&[u8]> for ContractPackageHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForContractHashError> {
        HashAddr::try_from(bytes)
            .map(ContractPackageHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

impl TryFrom<&Vec<u8>> for ContractPackageHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        HashAddr::try_from(bytes as &[u8])
            .map(ContractPackageHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

#[cfg(feature = "json-schema")]
impl JsonSchema for ContractPackageHash {
    fn schema_name() -> String {
        String::from("ContractPackageHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description =
            Some("The hash address of the contract package".to_string());
        schema_object.into()
    }
}

/// A enum to determine the lock status of the contract package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum ContractPackageStatus {
    /// The package is locked and cannot be versioned.
    Locked,
    /// The package is unlocked and can be versioned.
    Unlocked,
}

impl ContractPackageStatus {
    /// Create a new status flag based on a boolean value
    pub fn new(is_locked: bool) -> Self {
        if is_locked {
            ContractPackageStatus::Locked
        } else {
            ContractPackageStatus::Unlocked
        }
    }
}

impl Default for ContractPackageStatus {
    fn default() -> Self {
        Self::Unlocked
    }
}

impl ToBytes for ContractPackageStatus {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        match self {
            ContractPackageStatus::Unlocked => result.append(&mut false.to_bytes()?),
            ContractPackageStatus::Locked => result.append(&mut true.to_bytes()?),
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        match self {
            ContractPackageStatus::Unlocked => false.serialized_length(),
            ContractPackageStatus::Locked => true.serialized_length(),
        }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ContractPackageStatus::Locked => writer.push(u8::from(true)),
            ContractPackageStatus::Unlocked => writer.push(u8::from(false)),
        }
        Ok(())
    }
}

impl FromBytes for ContractPackageStatus {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (val, bytes) = bool::from_bytes(bytes)?;
        let status = ContractPackageStatus::new(val);
        Ok((status, bytes))
    }
}

/// Contract definition, metadata, and security container.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ContractPackage {
    /// Key used to add or disable versions
    access_key: URef,
    /// All versions (enabled & disabled)
    versions: ContractVersions,
    /// Disabled versions
    disabled_versions: DisabledVersions,
    /// Mapping maintaining the set of URefs associated with each "user
    /// group". This can be used to control access to methods in a particular
    /// version of the contract. A method is callable by any context which
    /// "knows" any of the URefs associated with the method's user group.
    groups: Groups,
    /// A flag that determines whether a contract is locked
    lock_status: ContractPackageStatus,
}

impl CLTyped for ContractPackage {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl ContractPackage {
    /// Create new `ContractPackage` (with no versions) from given access key.
    pub fn new(
        access_key: URef,
        versions: ContractVersions,
        disabled_versions: DisabledVersions,
        groups: Groups,
        lock_status: ContractPackageStatus,
    ) -> Self {
        ContractPackage {
            access_key,
            versions,
            disabled_versions,
            groups,
            lock_status,
        }
    }

    /// Get the access key for this contract.
    pub fn access_key(&self) -> URef {
        self.access_key
    }

    /// Get the group definitions for this contract.
    pub fn groups(&self) -> &Groups {
        &self.groups
    }

    /// Returns reference to all of this contract's versions.
    pub fn versions(&self) -> &ContractVersions {
        &self.versions
    }

    /// Returns mutable reference to all of this contract's versions (enabled and disabled).
    pub fn versions_mut(&mut self) -> &mut ContractVersions {
        &mut self.versions
    }

    /// Consumes the object and returns all of this contract's versions (enabled and disabled).
    pub fn take_versions(self) -> ContractVersions {
        self.versions
    }

    /// Returns all of this contract's disabled versions.
    pub fn disabled_versions(&self) -> &DisabledVersions {
        &self.disabled_versions
    }

    /// Returns mut reference to all of this contract's disabled versions.
    pub fn disabled_versions_mut(&mut self) -> &mut DisabledVersions {
        &mut self.disabled_versions
    }

    #[cfg(test)]
    fn next_contract_version_for(&self, protocol_version: ProtocolVersionMajor) -> ContractVersion {
        let current_version = self
            .versions
            .keys()
            .rev()
            .find_map(|&contract_version_key| {
                if contract_version_key.protocol_version_major() == protocol_version {
                    Some(contract_version_key.contract_version())
                } else {
                    None
                }
            })
            .unwrap_or(0);

        current_version + 1
    }

    #[cfg(test)]
    fn insert_contract_version(
        &mut self,
        protocol_version_major: ProtocolVersionMajor,
        contract_hash: ContractHash,
    ) -> ContractVersionKey {
        let contract_version = self.next_contract_version_for(protocol_version_major);
        let key = ContractVersionKey::new(protocol_version_major, contract_version);
        self.versions.insert(key, contract_hash);
        key
    }

    #[cfg(test)]
    fn groups_mut(&mut self) -> &mut Groups {
        &mut self.groups
    }
}

impl ToBytes for ContractPackage {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.access_key().write_bytes(&mut result)?;
        self.versions().write_bytes(&mut result)?;
        self.disabled_versions().write_bytes(&mut result)?;
        self.groups().write_bytes(&mut result)?;
        self.lock_status.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.access_key.serialized_length()
            + self.versions.serialized_length()
            + self.disabled_versions.serialized_length()
            + self.groups.serialized_length()
            + self.lock_status.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.access_key().write_bytes(writer)?;
        self.versions().write_bytes(writer)?;
        self.disabled_versions().write_bytes(writer)?;
        self.groups().write_bytes(writer)?;
        self.lock_status.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for ContractPackage {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (access_key, bytes) = URef::from_bytes(bytes)?;
        let (versions, bytes) = ContractVersions::from_bytes(bytes)?;
        let (disabled_versions, bytes) = DisabledVersions::from_bytes(bytes)?;
        let (groups, bytes) = Groups::from_bytes(bytes)?;
        let (lock_status, bytes) = ContractPackageStatus::from_bytes(bytes)?;
        let result = ContractPackage {
            access_key,
            versions,
            disabled_versions,
            groups,
            lock_status,
        };

        Ok((result, bytes))
    }
}

impl From<ContractPackage> for Package {
    fn from(value: ContractPackage) -> Self {
        let versions: BTreeMap<EntityVersionKey, AddressableEntityHash> = value
            .versions
            .into_iter()
            .map(|(version, contract_hash)| {
                let entity_version = EntityVersionKey::new(2, version.contract_version());
                let entity_hash: AddressableEntityHash =
                    AddressableEntityHash::new(contract_hash.value());
                (entity_version, entity_hash)
            })
            .collect();

        let disabled_versions = value
            .disabled_versions
            .into_iter()
            .map(|contract_versions| {
                EntityVersionKey::new(
                    contract_versions.protocol_version_major(),
                    contract_versions.contract_version(),
                )
            })
            .collect();

        let lock_status = if value.lock_status == ContractPackageStatus::Locked {
            PackageStatus::Locked
        } else {
            PackageStatus::Unlocked
        };

        Package::new(
            value.access_key,
            versions.into(),
            disabled_versions,
            value.groups,
            lock_status,
        )
    }
}

/// Methods and type signatures supported by a contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Contract {
    contract_package_hash: ContractPackageHash,
    contract_wasm_hash: ContractWasmHash,
    named_keys: NamedKeys,
    entry_points: EntryPoints,
    protocol_version: ProtocolVersion,
}

impl From<Contract>
    for (
        ContractPackageHash,
        ContractWasmHash,
        NamedKeys,
        EntryPoints,
        ProtocolVersion,
    )
{
    fn from(contract: Contract) -> Self {
        (
            contract.contract_package_hash,
            contract.contract_wasm_hash,
            contract.named_keys,
            contract.entry_points,
            contract.protocol_version,
        )
    }
}

impl Contract {
    /// `Contract` constructor.
    pub fn new(
        contract_package_hash: ContractPackageHash,
        contract_wasm_hash: ContractWasmHash,
        named_keys: NamedKeys,
        entry_points: EntryPoints,
        protocol_version: ProtocolVersion,
    ) -> Self {
        Contract {
            contract_package_hash,
            contract_wasm_hash,
            named_keys,
            entry_points,
            protocol_version,
        }
    }

    /// Hash for accessing contract package
    pub fn contract_package_hash(&self) -> ContractPackageHash {
        self.contract_package_hash
    }

    /// Hash for accessing contract WASM
    pub fn contract_wasm_hash(&self) -> ContractWasmHash {
        self.contract_wasm_hash
    }

    /// Checks whether there is a method with the given name
    pub fn has_entry_point(&self, name: &str) -> bool {
        self.entry_points.has_entry_point(name)
    }

    /// Returns the type signature for the given `method`.
    pub fn entry_point(&self, method: &str) -> Option<&EntryPoint> {
        self.entry_points.get(method)
    }

    /// Get the protocol version this header is targeting.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Adds new entry point
    pub fn add_entry_point<T: Into<String>>(&mut self, entry_point: EntryPoint) {
        self.entry_points.add_entry_point(entry_point);
    }

    /// Hash for accessing contract bytes
    pub fn contract_wasm_key(&self) -> Key {
        self.contract_wasm_hash.into()
    }

    /// Returns immutable reference to methods
    pub fn entry_points(&self) -> &EntryPoints {
        &self.entry_points
    }

    /// Takes `named_keys`
    pub fn take_named_keys(self) -> NamedKeys {
        self.named_keys
    }

    /// Returns a reference to `named_keys`
    pub fn named_keys(&self) -> &NamedKeys {
        &self.named_keys
    }

    /// Appends `keys` to `named_keys`
    pub fn named_keys_append(&mut self, keys: NamedKeys) {
        self.named_keys.append(keys);
    }

    /// Removes given named key.
    pub fn remove_named_key(&mut self, key: &str) -> Option<Key> {
        self.named_keys.remove(key)
    }

    /// Set protocol_version.
    pub fn set_protocol_version(&mut self, protocol_version: ProtocolVersion) {
        self.protocol_version = protocol_version;
    }

    /// Determines if `Contract` is compatible with a given `ProtocolVersion`.
    pub fn is_compatible_protocol_version(&self, protocol_version: ProtocolVersion) -> bool {
        self.protocol_version.value().major == protocol_version.value().major
    }
}

impl ToBytes for Contract {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.contract_package_hash().write_bytes(&mut result)?;
        self.contract_wasm_hash().write_bytes(&mut result)?;
        self.named_keys().write_bytes(&mut result)?;
        self.entry_points().write_bytes(&mut result)?;
        self.protocol_version().write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.entry_points)
            + ToBytes::serialized_length(&self.contract_package_hash)
            + ToBytes::serialized_length(&self.contract_wasm_hash)
            + ToBytes::serialized_length(&self.protocol_version)
            + ToBytes::serialized_length(&self.named_keys)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.contract_package_hash().write_bytes(writer)?;
        self.contract_wasm_hash().write_bytes(writer)?;
        self.named_keys().write_bytes(writer)?;
        self.entry_points().write_bytes(writer)?;
        self.protocol_version().write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for Contract {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (contract_package_hash, bytes) = FromBytes::from_bytes(bytes)?;
        let (contract_wasm_hash, bytes) = FromBytes::from_bytes(bytes)?;
        let (named_keys, bytes) = NamedKeys::from_bytes(bytes)?;
        let (entry_points, bytes) = EntryPoints::from_bytes(bytes)?;
        let (protocol_version, bytes) = ProtocolVersion::from_bytes(bytes)?;
        Ok((
            Contract {
                contract_package_hash,
                contract_wasm_hash,
                named_keys,
                entry_points,
                protocol_version,
            },
            bytes,
        ))
    }
}

impl Default for Contract {
    fn default() -> Self {
        Contract {
            named_keys: NamedKeys::default(),
            entry_points: EntryPoints::default(),
            contract_wasm_hash: [0; KEY_HASH_LENGTH].into(),
            contract_package_hash: [0; KEY_HASH_LENGTH].into(),
            protocol_version: ProtocolVersion::V1_0_0,
        }
    }
}

/// Default name for an entry point
pub const DEFAULT_ENTRY_POINT_NAME: &str = "call";

/// Default name for an installer entry point
pub const ENTRY_POINT_NAME_INSTALL: &str = "install";

/// Default name for an upgrade entry point
pub const UPGRADE_ENTRY_POINT_NAME: &str = "upgrade";

#[cfg(test)]
mod tests {

    use super::*;
    use crate::{AccessRights, EntryPointAccess, EntryPointType, Group, Parameter, URef};
    use alloc::borrow::ToOwned;

    const CONTRACT_HASH_V1: ContractHash = ContractHash::new([42; 32]);
    const CONTRACT_HASH_V2: ContractHash = ContractHash::new([84; 32]);

    fn make_contract_package() -> ContractPackage {
        let mut contract_package = ContractPackage::new(
            URef::new([0; 32], AccessRights::NONE),
            ContractVersions::default(),
            DisabledVersions::default(),
            Groups::default(),
            ContractPackageStatus::default(),
        );

        // add groups
        {
            let group_urefs = {
                let mut ret = BTreeSet::new();
                ret.insert(URef::new([1; 32], AccessRights::READ));
                ret
            };

            contract_package
                .groups_mut()
                .insert(Group::new("Group 1"), group_urefs.clone());

            contract_package
                .groups_mut()
                .insert(Group::new("Group 2"), group_urefs);
        }

        // add entry_points
        let _entry_points = {
            let mut ret = BTreeMap::new();
            let entrypoint = EntryPoint::new(
                "method0".to_string(),
                vec![],
                CLType::U32,
                EntryPointAccess::groups(&["Group 2"]),
                EntryPointType::Session,
            );
            ret.insert(entrypoint.name().to_owned(), entrypoint);
            let entrypoint = EntryPoint::new(
                "method1".to_string(),
                vec![Parameter::new("Foo", CLType::U32)],
                CLType::U32,
                EntryPointAccess::groups(&["Group 1"]),
                EntryPointType::Session,
            );
            ret.insert(entrypoint.name().to_owned(), entrypoint);
            ret
        };

        let _contract_package_hash = [41; 32];
        let _contract_wasm_hash = [43; 32];
        let _named_keys = NamedKeys::new();
        let protocol_version = ProtocolVersion::V1_0_0;

        let v1 = contract_package
            .insert_contract_version(protocol_version.value().major, CONTRACT_HASH_V1);
        let v2 = contract_package
            .insert_contract_version(protocol_version.value().major, CONTRACT_HASH_V2);

        assert!(v2 > v1);

        contract_package
    }

    #[test]
    fn roundtrip_serialization() {
        let contract_package = make_contract_package();
        let bytes = contract_package.to_bytes().expect("should serialize");
        let (decoded_package, rem) =
            ContractPackage::from_bytes(&bytes).expect("should deserialize");
        assert_eq!(contract_package, decoded_package);
        assert_eq!(rem.len(), 0);
    }

    #[test]
    fn contract_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash = HashAddr::try_from(&bytes[..]).expect("should create contract hash");
        let contract_hash = ContractHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_package_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash = HashAddr::try_from(&bytes[..]).expect("should create contract hash");
        let contract_hash = ContractPackageHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_hash_from_str() {
        let contract_hash = ContractHash([3; 32]);
        let encoded = contract_hash.to_formatted_string();
        let decoded = ContractHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(contract_hash, decoded);

        let invalid_prefix =
            "contract--0000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_str(invalid_prefix).is_err());

        let short_addr = "contract-00000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_str(short_addr).is_err());

        let long_addr =
            "contract-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_str(long_addr).is_err());

        let invalid_hex =
            "contract-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(ContractHash::from_formatted_str(invalid_hex).is_err());
    }

    #[test]
    fn contract_package_hash_from_str() {
        let contract_package_hash = ContractPackageHash([3; 32]);
        let encoded = contract_package_hash.to_formatted_string();
        let decoded = ContractPackageHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(contract_package_hash, decoded);

        let invalid_prefix =
            "contract-package0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            ContractPackageHash::from_formatted_str(invalid_prefix).unwrap_err(),
            FromStrError::InvalidPrefix
        ));

        let short_addr =
            "contract-package-00000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            ContractPackageHash::from_formatted_str(short_addr).unwrap_err(),
            FromStrError::Hash(_)
        ));

        let long_addr =
            "contract-package-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            ContractPackageHash::from_formatted_str(long_addr).unwrap_err(),
            FromStrError::Hash(_)
        ));

        let invalid_hex =
            "contract-package-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(matches!(
            ContractPackageHash::from_formatted_str(invalid_hex).unwrap_err(),
            FromStrError::Hex(_)
        ));
    }

    #[test]
    fn contract_package_hash_from_legacy_str() {
        let contract_package_hash = ContractPackageHash([3; 32]);
        let hex_addr = contract_package_hash.to_string();
        let legacy_encoded = format!("contract-package-wasm{}", hex_addr);
        let decoded_from_legacy = ContractPackageHash::from_formatted_str(&legacy_encoded)
            .expect("should accept legacy prefixed string");
        assert_eq!(
            contract_package_hash, decoded_from_legacy,
            "decoded_from_legacy should equal decoded"
        );

        let invalid_prefix =
            "contract-packagewasm0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            ContractPackageHash::from_formatted_str(invalid_prefix).unwrap_err(),
            FromStrError::InvalidPrefix
        ));

        let short_addr =
            "contract-package-wasm00000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            ContractPackageHash::from_formatted_str(short_addr).unwrap_err(),
            FromStrError::Hash(_)
        ));

        let long_addr =
            "contract-package-wasm000000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            ContractPackageHash::from_formatted_str(long_addr).unwrap_err(),
            FromStrError::Hash(_)
        ));

        let invalid_hex =
            "contract-package-wasm000000000000000000000000000000000000000000000000000000000000000g";
        assert!(matches!(
            ContractPackageHash::from_formatted_str(invalid_hex).unwrap_err(),
            FromStrError::Hex(_)
        ));
    }

    #[test]
    fn contract_hash_serde_roundtrip() {
        let contract_hash = ContractHash([255; 32]);
        let serialized = bincode::serialize(&contract_hash).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(contract_hash, deserialized)
    }

    #[test]
    fn contract_hash_json_roundtrip() {
        let contract_hash = ContractHash([255; 32]);
        let json_string = serde_json::to_string_pretty(&contract_hash).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(contract_hash, decoded)
    }

    #[test]
    fn contract_package_hash_serde_roundtrip() {
        let contract_hash = ContractPackageHash([255; 32]);
        let serialized = bincode::serialize(&contract_hash).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(contract_hash, deserialized)
    }

    #[test]
    fn contract_package_hash_json_roundtrip() {
        let contract_hash = ContractPackageHash([255; 32]);
        let json_string = serde_json::to_string_pretty(&contract_hash).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(contract_hash, decoded)
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 1024,
            .. ProptestConfig::default()
        })]

        #[test]
        fn test_value_contract(contract in gens::contract_arb()) {
            bytesrepr::test_serialization_roundtrip(&contract);
        }

        #[test]
        fn test_value_contract_package(contract_pkg in gens::contract_package_arb()) {
            bytesrepr::test_serialization_roundtrip(&contract_pkg);
        }
    }
}
