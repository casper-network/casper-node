//! Data types for supporting contract headers feature.
// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

mod account_hash;
pub mod action_thresholds;
mod action_type;
pub mod associated_keys;
mod error;
mod weight;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
    iter,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

pub use self::{
    account_hash::{AccountHash, ACCOUNT_HASH_FORMATTED_STRING_PREFIX, ACCOUNT_HASH_LENGTH},
    action_thresholds::ActionThresholds,
    action_type::ActionType,
    associated_keys::AssociatedKeys,
    error::{
        FromAccountHashStrError, SetThresholdFailure, TryFromIntError,
        TryFromSliceForAccountHashError,
    },
    weight::{Weight, WEIGHT_SERIALIZED_LENGTH},
};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH},
    checksummed_hex,
    contract_wasm::ContractWasmHash,
    crypto::{self, PublicKey},
    system::SystemContractType,
    uref,
    uref::URef,
    AccessRights, CLType, CLTyped, ContextAccessRights, HashAddr, Key, ProtocolVersion,
    BLAKE2B_DIGEST_LENGTH, KEY_HASH_LENGTH,
};

/// Maximum number of distinct user groups.
pub const MAX_GROUPS: u8 = 10;
/// Maximum number of URefs which can be assigned across all user groups.
pub const MAX_TOTAL_UREFS: usize = 100;

/// The tag for Contract Packages associated with Wasm stored on chain.
pub const PACKAGE_KIND_WASM_TAG: u8 = 0;
/// The tag for Contract Package associated with a native contract implementation.
pub const PACKAGE_KIND_SYSTEM_CONTRACT_TAG: u8 = 1;
/// The tag for Contract Package associated with an Account hash.
pub const PACKAGE_KIND_ACCOUNT_TAG: u8 = 2;
/// The tag for Contract Packages associated with legacy packages.
pub const PACKAGE_KIND_LEGACY_TAG: u8 = 3;

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
    /// Error when parsing an uref.
    URef(uref::FromStrError),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

// impl From<TryFromSliceForAccountHashError> for FromStrError {
//     fn from(error: TryFromSliceForAccountHashError) -> Self {
//         FromStrError::Account(error)
//     }
// }

impl From<TryFromSliceError> for FromStrError {
    fn from(error: TryFromSliceError) -> Self {
        FromStrError::Hash(error)
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
            // FromStrError::Account(error) => write!(f, "account from string error: {:?}", error),
            FromStrError::Hash(error) => write!(f, "hash from string error: {}", error),
            // FromStrError::AccountHash(error) => {
            //     write!(f, "account hash from string error: {:?}", error)
            // }
            FromStrError::URef(error) => write!(f, "uref from string error: {:?}", error),
            FromStrError::Account(error) => {
                write!(f, "account hash from string error: {:?}", error)
            }
        }
    }
}

/// A (labelled) "user group". Each method of a versioned contract may be
/// associated with one or more user groups which are allowed to call it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Group(String);

impl Group {
    /// Basic constructor
    pub fn new<T: Into<String>>(s: T) -> Self {
        Group(s.into())
    }

    /// Retrieves underlying name.
    pub fn value(&self) -> &str {
        &self.0
    }
}

impl From<Group> for String {
    fn from(group: Group) -> Self {
        group.0
    }
}

impl ToBytes for Group {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.value().write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for Group {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        String::from_bytes(bytes).map(|(label, bytes)| (Group(label), bytes))
    }
}

/// Automatically incremented value for a contract version within a major `ProtocolVersion`.
pub type ContractVersion = u32;

/// Within each discrete major `ProtocolVersion`, contract version resets to this value.
pub const CONTRACT_INITIAL_VERSION: ContractVersion = 1;

/// Major element of `ProtocolVersion` a `ContractVersion` is compatible with.
pub type ProtocolVersionMajor = u32;

/// Major element of `ProtocolVersion` combined with `ContractVersion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
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

/// Collection of named groups.
pub type Groups = BTreeMap<Group, BTreeSet<URef>>;

/// A newtype wrapping a `HashAddr` which references a [`AddressableEntity`] in the global state.
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
        schema_object.metadata().description =
            Some("The hex-encoded hash address of the contract".to_string());
        schema_object.into()
    }
}

/// A newtype wrapping a `HashAddr` which references a [`Package`] in the global state.
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

    /// Parses a `PublicKey` and outputs the corresponding account hash.
    pub fn from_public_key(
        public_key: &PublicKey,
        blake2b_hash_fn: impl Fn(Vec<u8>) -> [u8; BLAKE2B_DIGEST_LENGTH],
    ) -> Self {
        const SYSTEM_LOWERCASE: &str = "system";
        const ED25519_LOWERCASE: &str = "ed25519";
        const SECP256K1_LOWERCASE: &str = "secp256k1";

        let algorithm_name = match public_key {
            PublicKey::System => SYSTEM_LOWERCASE,
            PublicKey::Ed25519(_) => ED25519_LOWERCASE,
            PublicKey::Secp256k1(_) => SECP256K1_LOWERCASE,
        };
        let public_key_bytes: Vec<u8> = public_key.into();

        // Prepare preimage based on the public key parameters.
        let preimage = {
            let mut data = Vec::with_capacity(algorithm_name.len() + public_key_bytes.len() + 1);
            data.extend(algorithm_name.as_bytes());
            data.push(0);
            data.extend(public_key_bytes);
            data
        };
        // Hash the preimage data using blake2b256 and return it.
        let digest = blake2b_hash_fn(preimage);
        Self::new(digest)
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

impl From<&PublicKey> for ContractPackageHash {
    fn from(public_key: &PublicKey) -> Self {
        ContractPackageHash::from_public_key(public_key, crypto::blake2b)
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
            Some("The hex-encoded hash address of the contract package".to_string());
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
/// The type of contract package.
pub enum ContractPackageKind {
    /// Contract Packages associated with Wasm stored on chain.
    Wasm,
    /// Contract Package associated with a native contract implementation.
    System(SystemContractType),
    /// Contract Package associated with an Account hash.
    Account(AccountHash),
    /// Contract Packages from the previous format.
    #[default]
    Legacy,
}

impl ContractPackageKind {
    /// Returns the Account hash associated with a Contract Package based on the package kind.
    pub fn maybe_account_hash(&self) -> Option<AccountHash> {
        match self {
            Self::Account(account_hash) => Some(*account_hash),
            Self::Wasm | Self::System(_) | Self::Legacy => None,
        }
    }

    /// Returns the associated key set based on the Account hash set in the package kind.
    pub fn associated_keys(&self) -> AssociatedKeys {
        match self {
            Self::Account(account_hash) => AssociatedKeys::new(*account_hash, Weight::new(1)),
            Self::Wasm | Self::System(_) | Self::Legacy => AssociatedKeys::default(),
        }
    }

    pub fn is_system(&self) -> bool {
        match self {
            ContractPackageKind::System(_) => true,
            ContractPackageKind::Account(account_hash) => {
                *account_hash == PublicKey::System.to_account_hash()
            }
            _ => false,
        }
    }

    pub fn is_system_mint(&self) -> bool {
        match self {
            Self::System(SystemContractType::Mint) => true,
            _ => false,
        }
    }

    pub fn is_system_auction(&self) -> bool {
        match self {
            Self::System(SystemContractType::Auction) => true,
            _ => false,
        }
    }

    pub fn is_system_account(&self) -> bool {
        match self {
            Self::Account(account_hash) => {
                if *account_hash == PublicKey::System.to_account_hash() {
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}

impl ToBytes for ContractPackageKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        match self {
            ContractPackageKind::Wasm => {
                result.insert(0, PACKAGE_KIND_WASM_TAG);
            }
            ContractPackageKind::System(system_contract_type) => {
                result.insert(0, PACKAGE_KIND_SYSTEM_CONTRACT_TAG);
                result.extend_from_slice(&system_contract_type.to_bytes()?);
            }
            ContractPackageKind::Account(account_hash) => {
                result.insert(0, PACKAGE_KIND_ACCOUNT_TAG);
                result.extend_from_slice(&account_hash.to_bytes()?);
            }
            ContractPackageKind::Legacy => {
                result.insert(0, PACKAGE_KIND_LEGACY_TAG);
            }
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                ContractPackageKind::Wasm | ContractPackageKind::Legacy => 0,
                ContractPackageKind::System(system_contract_type) => {
                    system_contract_type.serialized_length()
                }
                ContractPackageKind::Account(account_hash) => account_hash.serialized_length(),
            }
    }
}

impl FromBytes for ContractPackageKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            PACKAGE_KIND_WASM_TAG => Ok((ContractPackageKind::Wasm, remainder)),
            PACKAGE_KIND_SYSTEM_CONTRACT_TAG => {
                let (system_contract_type, remainder) = SystemContractType::from_bytes(remainder)?;
                Ok((ContractPackageKind::System(system_contract_type), remainder))
            }
            PACKAGE_KIND_ACCOUNT_TAG => {
                let (account_hash, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((ContractPackageKind::Account(account_hash), remainder))
            }
            PACKAGE_KIND_LEGACY_TAG => Ok((ContractPackageKind::Legacy, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Contract definition, metadata, and security container.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct Package {
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

    contract_package_kind: ContractPackageKind,
}

impl CLTyped for Package {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl Package {
    /// Create new `ContractPackage` (with no versions) from given access key.
    pub fn new(
        access_key: URef,
        versions: ContractVersions,
        disabled_versions: DisabledVersions,
        groups: Groups,
        lock_status: ContractPackageStatus,
        contract_package_kind: ContractPackageKind,
    ) -> Self {
        Package {
            access_key,
            versions,
            disabled_versions,
            groups,
            lock_status,
            contract_package_kind,
        }
    }

    /// Get the access key for this contract.
    pub fn access_key(&self) -> URef {
        self.access_key
    }

    /// Get the mutable group definitions for this contract.
    pub fn groups_mut(&mut self) -> &mut Groups {
        &mut self.groups
    }

    /// Get the group definitions for this contract.
    pub fn groups(&self) -> &Groups {
        &self.groups
    }

    /// Adds new group to this contract.
    pub fn add_group(&mut self, group: Group, urefs: BTreeSet<URef>) {
        let v = self.groups.entry(group).or_insert_with(Default::default);
        v.extend(urefs)
    }

    /// Lookup the contract hash for a given contract version (if present)
    pub fn lookup_contract_hash(
        &self,
        contract_version_key: ContractVersionKey,
    ) -> Option<&ContractHash> {
        if !self.is_version_enabled(contract_version_key) {
            return None;
        }
        self.versions.get(&contract_version_key)
    }

    /// Checks if the given contract version exists and is available for use.
    pub fn is_version_enabled(&self, contract_version_key: ContractVersionKey) -> bool {
        !self.disabled_versions.contains(&contract_version_key)
            && self.versions.contains_key(&contract_version_key)
    }

    /// Returns `true` if the given contract hash exists and is enabled.
    pub fn is_contract_enabled(&self, contract_hash: &ContractHash) -> bool {
        match self.find_contract_version_key_by_hash(contract_hash) {
            Some(version_key) => !self.disabled_versions.contains(version_key),
            None => false,
        }
    }

    /// Insert a new contract version; the next sequential version number will be issued.
    pub fn insert_contract_version(
        &mut self,
        protocol_version_major: ProtocolVersionMajor,
        contract_hash: ContractHash,
    ) -> ContractVersionKey {
        let contract_version = self.next_contract_version_for(protocol_version_major);
        let key = ContractVersionKey::new(protocol_version_major, contract_version);
        self.versions.insert(key, contract_hash);
        key
    }

    /// Disable the contract version corresponding to the given hash (if it exists).
    pub fn disable_contract_version(&mut self, contract_hash: ContractHash) -> Result<(), Error> {
        let contract_version_key = self
            .versions
            .iter()
            .filter_map(|(k, v)| if *v == contract_hash { Some(*k) } else { None })
            .next()
            .ok_or(Error::ContractNotFound)?;

        if !self.disabled_versions.contains(&contract_version_key) {
            self.disabled_versions.insert(contract_version_key);
        }

        Ok(())
    }

    fn find_contract_version_key_by_hash(
        &self,
        contract_hash: &ContractHash,
    ) -> Option<&ContractVersionKey> {
        self.versions
            .iter()
            .filter_map(|(k, v)| if v == contract_hash { Some(k) } else { None })
            .next()
    }

    /// Returns reference to all of this contract's versions.
    pub fn versions(&self) -> &ContractVersions {
        &self.versions
    }

    /// Returns all of this contract's enabled contract versions.
    pub fn enabled_versions(&self) -> ContractVersions {
        let mut ret = ContractVersions::new();
        for version in &self.versions {
            if !self.is_version_enabled(*version.0) {
                continue;
            }
            ret.insert(*version.0, *version.1);
        }
        ret
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

    /// Removes a group from this contract (if it exists).
    pub fn remove_group(&mut self, group: &Group) -> bool {
        self.groups.remove(group).is_some()
    }

    /// Gets the next available contract version for the given protocol version
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

    /// Return the contract version key for the newest enabled contract version.
    pub fn current_contract_version(&self) -> Option<ContractVersionKey> {
        self.enabled_versions().keys().next_back().copied()
    }

    /// Return the contract hash for the newest enabled contract version.
    pub fn current_contract_hash(&self) -> Option<ContractHash> {
        self.enabled_versions().values().next_back().copied()
    }

    /// Return the lock status of the contract package.
    pub fn is_locked(&self) -> bool {
        match self.lock_status {
            ContractPackageStatus::Unlocked => false,
            ContractPackageStatus::Locked => true,
        }
    }

    /// Return the package status itself
    pub fn get_lock_status(&self) -> ContractPackageStatus {
        self.lock_status.clone()
    }

    /// Returns the kind of Contract Package.
    pub fn get_contract_package_kind(&self) -> ContractPackageKind {
        self.contract_package_kind.clone()
    }

    /// Returns whether the contract package is of the legacy format.
    pub fn is_legacy(&self) -> bool {
        matches!(self.contract_package_kind, ContractPackageKind::Legacy)
    }

    /// Update the contract package kind.
    pub fn update_package_kind(&mut self, new_package_kind: ContractPackageKind) {
        self.contract_package_kind = new_package_kind
    }
}

impl ToBytes for Package {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.access_key().write_bytes(&mut result)?;
        self.versions().write_bytes(&mut result)?;
        self.disabled_versions().write_bytes(&mut result)?;
        self.groups().write_bytes(&mut result)?;
        self.lock_status.write_bytes(&mut result)?;
        self.contract_package_kind.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.access_key.serialized_length()
            + self.versions.serialized_length()
            + self.disabled_versions.serialized_length()
            + self.groups.serialized_length()
            + self.lock_status.serialized_length()
            + self.contract_package_kind.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.access_key().write_bytes(writer)?;
        self.versions().write_bytes(writer)?;
        self.disabled_versions().write_bytes(writer)?;
        self.groups().write_bytes(writer)?;
        self.lock_status.write_bytes(writer)?;
        self.contract_package_kind.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for Package {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (access_key, bytes) = URef::from_bytes(bytes)?;
        let (versions, bytes) = ContractVersions::from_bytes(bytes)?;
        let (disabled_versions, bytes) = DisabledVersions::from_bytes(bytes)?;
        let (groups, bytes) = Groups::from_bytes(bytes)?;
        let (lock_status, bytes) = ContractPackageStatus::from_bytes(bytes)?;
        let (contract_package_kind, bytes) =
            ContractPackageKind::from_bytes(bytes).unwrap_or_default();
        let result = Package {
            access_key,
            versions,
            disabled_versions,
            groups,
            lock_status,
            contract_package_kind,
        };

        Ok((result, bytes))
    }
}

/// Errors that can occur while adding a new [`AccountHash`] to an account's associated keys map.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[repr(i32)]
#[non_exhaustive]
pub enum AddKeyFailure {
    /// There are already maximum [`AccountHash`]s associated with the given account.
    MaxKeysLimit = 1,
    /// The given [`AccountHash`] is already associated with the given account.
    DuplicateKey = 2,
    /// Caller doesn't have sufficient permissions to associate a new [`AccountHash`] with the
    /// given account.
    PermissionDenied = 3,
}

impl Display for AddKeyFailure {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            AddKeyFailure::MaxKeysLimit => formatter.write_str(
                "Unable to add new associated key because maximum amount of keys is reached",
            ),
            AddKeyFailure::DuplicateKey => formatter
                .write_str("Unable to add new associated key because given key already exists"),
            AddKeyFailure::PermissionDenied => formatter
                .write_str("Unable to add new associated key due to insufficient permissions"),
        }
    }
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<i32> for AddKeyFailure {
    type Error = TryFromIntError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            d if d == AddKeyFailure::MaxKeysLimit as i32 => Ok(AddKeyFailure::MaxKeysLimit),
            d if d == AddKeyFailure::DuplicateKey as i32 => Ok(AddKeyFailure::DuplicateKey),
            d if d == AddKeyFailure::PermissionDenied as i32 => Ok(AddKeyFailure::PermissionDenied),
            _ => Err(TryFromIntError(())),
        }
    }
}

/// Errors that can occur while removing a [`AccountHash`] from an account's associated keys map.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(i32)]
#[non_exhaustive]
pub enum RemoveKeyFailure {
    /// The given [`AccountHash`] is not associated with the given account.
    MissingKey = 1,
    /// Caller doesn't have sufficient permissions to remove an associated [`AccountHash`] from the
    /// given account.
    PermissionDenied = 2,
    /// Removing the given associated [`AccountHash`] would cause the total weight of all remaining
    /// `AccountHash`s to fall below one of the action thresholds for the given account.
    ThresholdViolation = 3,
}

impl Display for RemoveKeyFailure {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            RemoveKeyFailure::MissingKey => {
                formatter.write_str("Unable to remove a key that does not exist")
            }
            RemoveKeyFailure::PermissionDenied => formatter
                .write_str("Unable to remove associated key due to insufficient permissions"),
            RemoveKeyFailure::ThresholdViolation => formatter.write_str(
                "Unable to remove a key which would violate action threshold constraints",
            ),
        }
    }
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<i32> for RemoveKeyFailure {
    type Error = TryFromIntError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            d if d == RemoveKeyFailure::MissingKey as i32 => Ok(RemoveKeyFailure::MissingKey),
            d if d == RemoveKeyFailure::PermissionDenied as i32 => {
                Ok(RemoveKeyFailure::PermissionDenied)
            }
            d if d == RemoveKeyFailure::ThresholdViolation as i32 => {
                Ok(RemoveKeyFailure::ThresholdViolation)
            }
            _ => Err(TryFromIntError(())),
        }
    }
}

/// Errors that can occur while updating the [`Weight`] of a [`AccountHash`] in an account's
/// associated keys map.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[repr(i32)]
#[non_exhaustive]
pub enum UpdateKeyFailure {
    /// The given [`AccountHash`] is not associated with the given account.
    MissingKey = 1,
    /// Caller doesn't have sufficient permissions to update an associated [`AccountHash`] from the
    /// given account.
    PermissionDenied = 2,
    /// Updating the [`Weight`] of the given associated [`AccountHash`] would cause the total
    /// weight of all `AccountHash`s to fall below one of the action thresholds for the given
    /// account.
    ThresholdViolation = 3,
}

impl Display for UpdateKeyFailure {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            UpdateKeyFailure::MissingKey => formatter.write_str(
                "Unable to update the value under an associated key that does not exist",
            ),
            UpdateKeyFailure::PermissionDenied => formatter
                .write_str("Unable to update associated key due to insufficient permissions"),
            UpdateKeyFailure::ThresholdViolation => formatter.write_str(
                "Unable to update weight that would fall below any of action thresholds",
            ),
        }
    }
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<i32> for UpdateKeyFailure {
    type Error = TryFromIntError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            d if d == UpdateKeyFailure::MissingKey as i32 => Ok(UpdateKeyFailure::MissingKey),
            d if d == UpdateKeyFailure::PermissionDenied as i32 => {
                Ok(UpdateKeyFailure::PermissionDenied)
            }
            d if d == UpdateKeyFailure::ThresholdViolation as i32 => {
                Ok(UpdateKeyFailure::ThresholdViolation)
            }
            _ => Err(TryFromIntError(())),
        }
    }
}

/// Type alias for a container used inside [`EntryPoints`].
pub type EntryPointsMap = BTreeMap<String, EntryPoint>;

/// Collection of named entry points
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct EntryPoints(EntryPointsMap);

impl Default for EntryPoints {
    fn default() -> Self {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::default();
        entry_points.add_entry_point(entry_point);
        entry_points
    }
}

impl ToBytes for EntryPoints {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for EntryPoints {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (entry_points_map, rem) = EntryPointsMap::from_bytes(bytes)?;
        Ok((EntryPoints(entry_points_map), rem))
    }
}

impl EntryPoints {
    /// Creates empty instance of [`EntryPoints`].
    pub fn new() -> EntryPoints {
        EntryPoints(EntryPointsMap::new())
    }

    /// Adds new [`EntryPoint`].
    pub fn add_entry_point(&mut self, entry_point: EntryPoint) {
        self.0.insert(entry_point.name().to_string(), entry_point);
    }

    /// Checks if given [`EntryPoint`] exists.
    pub fn has_entry_point(&self, entry_point_name: &str) -> bool {
        self.0.contains_key(entry_point_name)
    }

    /// Gets an existing [`EntryPoint`] by its name.
    pub fn get(&self, entry_point_name: &str) -> Option<&EntryPoint> {
        self.0.get(entry_point_name)
    }

    /// Returns iterator for existing entry point names.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    /// Takes all entry points.
    pub fn take_entry_points(self) -> Vec<EntryPoint> {
        self.0.into_values().collect()
    }

    /// Returns the length of the entry points
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Checks if the `EntryPoints` is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<EntryPoint>> for EntryPoints {
    fn from(entry_points: Vec<EntryPoint>) -> EntryPoints {
        let entries = entry_points
            .into_iter()
            .map(|entry_point| (String::from(entry_point.name()), entry_point))
            .collect();
        EntryPoints(entries)
    }
}

/// Collection of named keys
pub type NamedKeys = BTreeMap<String, Key>;

/// Represents an Account in the global state.
#[derive(PartialEq, Eq, Clone, Debug, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct Account {
    account_hash: AccountHash,
    named_keys: NamedKeys,
    main_purse: URef,
    associated_keys: AssociatedKeys,
    action_thresholds: ActionThresholds,
}

impl Account {
    /// Creates a new account.
    pub fn new(
        account_hash: AccountHash,
        named_keys: NamedKeys,
        main_purse: URef,
        associated_keys: AssociatedKeys,
        action_thresholds: ActionThresholds,
    ) -> Self {
        Account {
            account_hash,
            named_keys,
            main_purse,
            associated_keys,
            action_thresholds,
        }
    }

    /// An Account constructor with presets for associated_keys and action_thresholds.
    ///
    /// An account created with this method is valid and can be used as the target of a transaction.
    /// It will be created with an [`AssociatedKeys`] with a [`Weight`] of 1, and a default
    /// [`ActionThresholds`].
    pub fn create(account: AccountHash, named_keys: NamedKeys, main_purse: URef) -> Self {
        let associated_keys = AssociatedKeys::new(account, Weight::new(1));
        let action_thresholds: ActionThresholds = Default::default();
        Account::new(
            account,
            named_keys,
            main_purse,
            associated_keys,
            action_thresholds,
        )
    }

    /// Extracts the access rights from the named keys and main purse of the account.
    pub fn extract_access_rights(&self) -> ContextAccessRights {
        let urefs_iter = self
            .named_keys
            .values()
            .filter_map(|key| key.as_uref().copied())
            .chain(iter::once(self.main_purse));
        ContextAccessRights::new(Key::from(self.account_hash), urefs_iter)
    }

    /// Appends named keys to an account's named_keys field.
    pub fn named_keys_append(&mut self, keys: &mut NamedKeys) {
        self.named_keys.append(keys);
    }

    /// Returns named keys.
    pub fn named_keys(&self) -> &NamedKeys {
        &self.named_keys
    }

    /// Returns a mutable reference to named keys.
    pub fn named_keys_mut(&mut self) -> &mut NamedKeys {
        &mut self.named_keys
    }

    /// Returns account hash.
    pub fn account_hash(&self) -> AccountHash {
        self.account_hash
    }

    /// Returns main purse.
    pub fn main_purse(&self) -> URef {
        self.main_purse
    }

    /// Returns an [`AccessRights::ADD`]-only version of the main purse's [`URef`].
    pub fn main_purse_add_only(&self) -> URef {
        URef::new(self.main_purse.addr(), AccessRights::ADD)
    }

    /// Returns associated keys.
    pub fn associated_keys(&self) -> &AssociatedKeys {
        &self.associated_keys
    }

    /// Returns action thresholds.
    pub fn action_thresholds(&self) -> &ActionThresholds {
        &self.action_thresholds
    }

    /// Adds an associated key to an account.
    pub fn add_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), AddKeyFailure> {
        self.associated_keys.add_key(account_hash, weight)
    }

    /// Checks if removing given key would properly satisfy thresholds.
    fn can_remove_key(&self, account_hash: AccountHash) -> bool {
        let total_weight_without = self
            .associated_keys
            .total_keys_weight_excluding(account_hash);

        // Returns true if the total weight calculated without given public key would be greater or
        // equal to all of the thresholds.
        total_weight_without >= *self.action_thresholds().deployment()
            && total_weight_without >= *self.action_thresholds().key_management()
    }

    /// Checks if adding a weight to a sum of all weights excluding the given key would make the
    /// resulting value to fall below any of the thresholds on account.
    fn can_update_key(&self, account_hash: AccountHash, weight: Weight) -> bool {
        // Calculates total weight of all keys excluding the given key
        let total_weight = self
            .associated_keys
            .total_keys_weight_excluding(account_hash);

        // Safely calculate new weight by adding the updated weight
        let new_weight = total_weight.value().saturating_add(weight.value());

        // Returns true if the new weight would be greater or equal to all of
        // the thresholds.
        new_weight >= self.action_thresholds().deployment().value()
            && new_weight >= self.action_thresholds().key_management().value()
    }

    /// Removes an associated key from an account.
    ///
    /// Verifies that removing the key will not cause the remaining weight to fall below any action
    /// thresholds.
    pub fn remove_associated_key(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<(), RemoveKeyFailure> {
        if self.associated_keys.contains_key(&account_hash) {
            // Check if removing this weight would fall below thresholds
            if !self.can_remove_key(account_hash) {
                return Err(RemoveKeyFailure::ThresholdViolation);
            }
        }
        self.associated_keys.remove_key(&account_hash)
    }

    /// Updates an associated key.
    ///
    /// Returns an error if the update would result in a violation of the key management thresholds.
    pub fn update_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), UpdateKeyFailure> {
        if let Some(current_weight) = self.associated_keys.get(&account_hash) {
            if weight < *current_weight {
                // New weight is smaller than current weight
                if !self.can_update_key(account_hash, weight) {
                    return Err(UpdateKeyFailure::ThresholdViolation);
                }
            }
        }
        self.associated_keys.update_key(account_hash, weight)
    }

    /// Sets new action threshold for a given action type for the account.
    ///
    /// Returns an error if the new action threshold weight is greater than the total weight of the
    /// account's associated keys.
    pub fn set_action_threshold(
        &mut self,
        action_type: ActionType,
        weight: Weight,
    ) -> Result<(), SetThresholdFailure> {
        // Verify if new threshold weight exceeds total weight of all associated
        // keys.
        self.can_set_threshold(weight)?;
        // Set new weight for given action
        self.action_thresholds.set_threshold(action_type, weight)
    }

    /// Verifies if user can set action threshold.
    pub fn can_set_threshold(&self, new_threshold: Weight) -> Result<(), SetThresholdFailure> {
        let total_weight = self.associated_keys.total_keys_weight();
        if new_threshold > total_weight {
            return Err(SetThresholdFailure::InsufficientTotalWeight);
        }
        Ok(())
    }

    /// Checks whether all authorization keys are associated with this account.
    pub fn can_authorize(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        !authorization_keys.is_empty()
            && authorization_keys
                .iter()
                .all(|e| self.associated_keys.contains_key(e))
    }

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to deploy threshold.
    pub fn can_deploy_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        total_weight >= *self.action_thresholds().deployment()
    }

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to key management threshold.
    pub fn can_manage_keys_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        total_weight >= *self.action_thresholds().key_management()
    }
}

impl ToBytes for Account {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.account_hash.write_bytes(&mut result)?;
        self.named_keys.write_bytes(&mut result)?;
        self.main_purse.write_bytes(&mut result)?;
        self.associated_keys.write_bytes(&mut result)?;
        self.action_thresholds.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.account_hash.serialized_length()
            + self.named_keys.serialized_length()
            + self.main_purse.serialized_length()
            + self.associated_keys.serialized_length()
            + self.action_thresholds.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.account_hash.write_bytes(writer)?;
        self.named_keys.write_bytes(writer)?;
        self.main_purse.write_bytes(writer)?;
        self.associated_keys.write_bytes(writer)?;
        self.action_thresholds.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for Account {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (account_hash, rem) = AccountHash::from_bytes(bytes)?;
        let (named_keys, rem) = NamedKeys::from_bytes(rem)?;
        let (main_purse, rem) = URef::from_bytes(rem)?;
        let (associated_keys, rem) = AssociatedKeys::from_bytes(rem)?;
        let (action_thresholds, rem) = ActionThresholds::from_bytes(rem)?;
        Ok((
            Account {
                account_hash,
                named_keys,
                main_purse,
                associated_keys,
                action_thresholds,
            },
            rem,
        ))
    }
}

/// Methods and type signatures supported by a contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct Contract {
    contract_package_hash: ContractPackageHash,
    contract_wasm_hash: ContractWasmHash,
    named_keys: NamedKeys,
    entry_points: EntryPoints,
    protocol_version: ProtocolVersion,
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

    /// Returns a reference to `named_keys`
    pub fn named_keys(&self) -> &NamedKeys {
        &self.named_keys
    }

    /// Returns immutable reference to methods
    pub fn entry_points(&self) -> &EntryPoints {
        &self.entry_points
    }

    /// Set protocol_version.
    pub fn set_protocol_version(&mut self, protocol_version: ProtocolVersion) {
        self.protocol_version = protocol_version;
    }
}

/// Methods and type signatures supported by a contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct AddressableEntity {
    contract_package_hash: ContractPackageHash,
    contract_wasm_hash: ContractWasmHash,
    named_keys: NamedKeys,
    entry_points: EntryPoints,
    protocol_version: ProtocolVersion,
    main_purse: URef,
    associated_keys: AssociatedKeys,
    action_thresholds: ActionThresholds,
}

impl From<AddressableEntity>
    for (
        ContractPackageHash,
        ContractWasmHash,
        NamedKeys,
        EntryPoints,
        ProtocolVersion,
        URef,
        AssociatedKeys,
        ActionThresholds,
    )
{
    fn from(contract: AddressableEntity) -> Self {
        (
            contract.contract_package_hash,
            contract.contract_wasm_hash,
            contract.named_keys,
            contract.entry_points,
            contract.protocol_version,
            contract.main_purse,
            contract.associated_keys,
            contract.action_thresholds,
        )
    }
}

impl AddressableEntity {
    /// `Contract` constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        contract_package_hash: ContractPackageHash,
        contract_wasm_hash: ContractWasmHash,
        named_keys: NamedKeys,
        entry_points: EntryPoints,
        protocol_version: ProtocolVersion,
        main_purse: URef,
        associated_keys: AssociatedKeys,
        action_thresholds: ActionThresholds,
    ) -> Self {
        AddressableEntity {
            contract_package_hash,
            contract_wasm_hash,
            named_keys,
            entry_points,
            protocol_version,
            main_purse,
            action_thresholds,
            associated_keys,
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

    /// Returns main purse.
    pub fn main_purse(&self) -> URef {
        self.main_purse
    }

    /// Returns an [`AccessRights::ADD`]-only version of the main purse's [`URef`].
    pub fn main_purse_add_only(&self) -> URef {
        URef::new(self.main_purse.addr(), AccessRights::ADD)
    }

    /// Returns associated keys.
    pub fn associated_keys(&self) -> &AssociatedKeys {
        &self.associated_keys
    }

    /// Returns action thresholds.
    pub fn action_thresholds(&self) -> &ActionThresholds {
        &self.action_thresholds
    }

    /// Adds an associated key to an account.
    pub fn add_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), AddKeyFailure> {
        self.associated_keys.add_key(account_hash, weight)
    }

    /// Checks if removing given key would properly satisfy thresholds.
    fn can_remove_key(&self, account_hash: AccountHash) -> bool {
        let total_weight_without = self
            .associated_keys
            .total_keys_weight_excluding(account_hash);

        // Returns true if the total weight calculated without given public key would be greater or
        // equal to all of the thresholds.
        total_weight_without >= *self.action_thresholds().deployment()
            && total_weight_without >= *self.action_thresholds().key_management()
    }

    /// Checks if adding a weight to a sum of all weights excluding the given key would make the
    /// resulting value to fall below any of the thresholds on account.
    fn can_update_key(&self, account_hash: AccountHash, weight: Weight) -> bool {
        // Calculates total weight of all keys excluding the given key
        let total_weight = self
            .associated_keys
            .total_keys_weight_excluding(account_hash);

        // Safely calculate new weight by adding the updated weight
        let new_weight = total_weight.value().saturating_add(weight.value());

        // Returns true if the new weight would be greater or equal to all of
        // the thresholds.
        new_weight >= self.action_thresholds().deployment().value()
            && new_weight >= self.action_thresholds().key_management().value()
    }

    /// Removes an associated key from an account.
    ///
    /// Verifies that removing the key will not cause the remaining weight to fall below any action
    /// thresholds.
    pub fn remove_associated_key(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<(), RemoveKeyFailure> {
        if self.associated_keys.contains_key(&account_hash) {
            // Check if removing this weight would fall below thresholds
            if !self.can_remove_key(account_hash) {
                return Err(RemoveKeyFailure::ThresholdViolation);
            }
        }
        self.associated_keys.remove_key(&account_hash)
    }

    /// Updates an associated key.
    ///
    /// Returns an error if the update would result in a violation of the key management thresholds.
    pub fn update_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), UpdateKeyFailure> {
        if let Some(current_weight) = self.associated_keys.get(&account_hash) {
            if weight < *current_weight {
                // New weight is smaller than current weight
                if !self.can_update_key(account_hash, weight) {
                    return Err(UpdateKeyFailure::ThresholdViolation);
                }
            }
        }
        self.associated_keys.update_key(account_hash, weight)
    }

    /// Sets new action threshold for a given action type for the account.
    ///
    /// Returns an error if the new action threshold weight is greater than the total weight of the
    /// account's associated keys.
    pub fn set_action_threshold(
        &mut self,
        action_type: ActionType,
        weight: Weight,
    ) -> Result<(), SetThresholdFailure> {
        // Verify if new threshold weight exceeds total weight of all associated
        // keys.
        self.can_set_threshold(weight)?;
        // Set new weight for given action
        self.action_thresholds.set_threshold(action_type, weight)
    }

    /// Verifies if user can set action threshold.
    pub fn can_set_threshold(&self, new_threshold: Weight) -> Result<(), SetThresholdFailure> {
        let total_weight = self.associated_keys.total_keys_weight();
        if new_threshold > total_weight {
            return Err(SetThresholdFailure::InsufficientTotalWeight);
        }
        Ok(())
    }

    /// Checks whether all authorization keys are associated with this account.
    pub fn can_authorize(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        !authorization_keys.is_empty()
            && authorization_keys
                .iter()
                .all(|e| self.associated_keys.contains_key(e))
    }

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to deploy threshold.
    pub fn can_deploy_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        total_weight >= *self.action_thresholds().deployment()
    }

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to key management threshold.
    pub fn can_manage_keys_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        total_weight >= *self.action_thresholds().key_management()
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
    pub fn named_keys_append(&mut self, keys: &mut NamedKeys) {
        self.named_keys.append(keys);
    }

    /// Removes given named key.
    pub fn remove_named_key(&mut self, key: &str) -> Option<Key> {
        self.named_keys.remove(key)
    }

    /// Returns a mutable reference to named keys.
    pub fn named_keys_mut(&mut self) -> &mut NamedKeys {
        &mut self.named_keys
    }

    /// Set protocol_version.
    pub fn set_protocol_version(&mut self, protocol_version: ProtocolVersion) {
        self.protocol_version = protocol_version;
    }

    /// Determines if `Contract` is compatible with a given `ProtocolVersion`.
    pub fn is_compatible_protocol_version(&self, protocol_version: ProtocolVersion) -> bool {
        self.protocol_version.value().major == protocol_version.value().major
    }

    /// Extracts the access rights from the named keys of the contract.
    pub fn extract_access_rights(&self, contract_hash: ContractHash) -> ContextAccessRights {
        let urefs_iter = self
            .named_keys
            .values()
            .filter_map(|key| key.as_uref().copied())
            .chain(iter::once(self.main_purse));
        ContextAccessRights::new(contract_hash.into(), urefs_iter)
    }
}

impl ToBytes for Contract {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.contract_package_hash.write_bytes(&mut result)?;
        self.contract_wasm_hash.write_bytes(&mut result)?;
        self.named_keys.write_bytes(&mut result)?;
        self.entry_points.write_bytes(&mut result)?;
        self.protocol_version.write_bytes(&mut result)?;
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
        self.contract_package_hash.write_bytes(writer)?;
        self.contract_wasm_hash.write_bytes(writer)?;
        self.named_keys.write_bytes(writer)?;
        self.entry_points.write_bytes(writer)?;
        self.protocol_version.write_bytes(writer)?;
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

impl ToBytes for AddressableEntity {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.contract_package_hash().write_bytes(&mut result)?;
        self.contract_wasm_hash().write_bytes(&mut result)?;
        self.named_keys().write_bytes(&mut result)?;
        self.entry_points().write_bytes(&mut result)?;
        self.protocol_version().write_bytes(&mut result)?;
        self.main_purse().write_bytes(&mut result)?;
        self.associated_keys().write_bytes(&mut result)?;
        self.action_thresholds().write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.entry_points)
            + ToBytes::serialized_length(&self.contract_package_hash)
            + ToBytes::serialized_length(&self.contract_wasm_hash)
            + ToBytes::serialized_length(&self.protocol_version)
            + ToBytes::serialized_length(&self.named_keys)
            + ToBytes::serialized_length(&self.main_purse)
            + ToBytes::serialized_length(&self.associated_keys)
            + ToBytes::serialized_length(&self.action_thresholds)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.contract_package_hash().write_bytes(writer)?;
        self.contract_wasm_hash().write_bytes(writer)?;
        self.named_keys().write_bytes(writer)?;
        self.entry_points().write_bytes(writer)?;
        self.protocol_version().write_bytes(writer)?;
        self.main_purse().write_bytes(writer)?;
        self.associated_keys().write_bytes(writer)?;
        self.action_thresholds().write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for AddressableEntity {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (contract_package_hash, bytes) = FromBytes::from_bytes(bytes)?;
        let (contract_wasm_hash, bytes) = FromBytes::from_bytes(bytes)?;
        let (named_keys, bytes) = NamedKeys::from_bytes(bytes)?;
        let (entry_points, bytes) = EntryPoints::from_bytes(bytes)?;
        let (protocol_version, bytes) = ProtocolVersion::from_bytes(bytes)?;
        let (main_purse, bytes) = URef::from_bytes(bytes)?;
        let (associated_keys, bytes) = AssociatedKeys::from_bytes(bytes)?;
        let (action_thresholds, bytes) = ActionThresholds::from_bytes(bytes)?;
        Ok((
            AddressableEntity {
                contract_package_hash,
                contract_wasm_hash,
                named_keys,
                entry_points,
                protocol_version,
                main_purse,
                associated_keys,
                action_thresholds,
            },
            bytes,
        ))
    }
}

impl Default for AddressableEntity {
    fn default() -> Self {
        AddressableEntity {
            named_keys: NamedKeys::default(),
            entry_points: EntryPoints::default(),
            contract_wasm_hash: [0; KEY_HASH_LENGTH].into(),
            contract_package_hash: [0; KEY_HASH_LENGTH].into(),
            protocol_version: ProtocolVersion::V1_0_0,
            main_purse: URef::default(),
            action_thresholds: ActionThresholds::default(),
            associated_keys: AssociatedKeys::default(),
        }
    }
}

impl From<Contract> for AddressableEntity {
    fn from(value: Contract) -> Self {
        AddressableEntity::new(
            value.contract_package_hash(),
            value.contract_wasm_hash(),
            value.named_keys().clone(),
            value.entry_points().clone(),
            value.protocol_version(),
            URef::default(),
            AssociatedKeys::default(),
            ActionThresholds::default(),
        )
    }
}

/// Context of method execution
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntryPointType {
    /// Runs as session code
    Session = 0,
    /// Runs within contract's context
    Contract = 1,
}

impl ToBytes for EntryPointType {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        (*self as u8).to_bytes()
    }

    fn serialized_length(&self) -> usize {
        1
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(*self as u8);
        Ok(())
    }
}

impl FromBytes for EntryPointType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, bytes) = u8::from_bytes(bytes)?;
        match value {
            0 => Ok((EntryPointType::Session, bytes)),
            1 => Ok((EntryPointType::Contract, bytes)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Default name for an entry point
pub const DEFAULT_ENTRY_POINT_NAME: &str = "call";

/// Default name for an installer entry point
pub const ENTRY_POINT_NAME_INSTALL: &str = "install";

/// Default name for an upgrade entry point
pub const UPGRADE_ENTRY_POINT_NAME: &str = "upgrade";

/// Collection of entry point parameters.
pub type Parameters = Vec<Parameter>;

/// Type signature of a method. Order of arguments matter since can be
/// referenced by index as well as name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EntryPoint {
    name: String,
    args: Parameters,
    ret: CLType,
    access: EntryPointAccess,
    entry_point_type: EntryPointType,
}

impl From<EntryPoint> for (String, Parameters, CLType, EntryPointAccess, EntryPointType) {
    fn from(entry_point: EntryPoint) -> Self {
        (
            entry_point.name,
            entry_point.args,
            entry_point.ret,
            entry_point.access,
            entry_point.entry_point_type,
        )
    }
}

impl EntryPoint {
    /// `EntryPoint` constructor.
    pub fn new<T: Into<String>>(
        name: T,
        args: Parameters,
        ret: CLType,
        access: EntryPointAccess,
        entry_point_type: EntryPointType,
    ) -> Self {
        EntryPoint {
            name: name.into(),
            args,
            ret,
            access,
            entry_point_type,
        }
    }

    /// Create a default [`EntryPoint`] with specified name.
    pub fn default_with_name<T: Into<String>>(name: T) -> Self {
        EntryPoint {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Get name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get access enum.
    pub fn access(&self) -> &EntryPointAccess {
        &self.access
    }

    /// Get the arguments for this method.
    pub fn args(&self) -> &[Parameter] {
        self.args.as_slice()
    }

    /// Get the return type.
    pub fn ret(&self) -> &CLType {
        &self.ret
    }

    /// Obtains entry point
    pub fn entry_point_type(&self) -> EntryPointType {
        self.entry_point_type
    }
}

impl Default for EntryPoint {
    /// constructor for a public session `EntryPoint` that takes no args and returns `Unit`
    fn default() -> Self {
        EntryPoint {
            name: DEFAULT_ENTRY_POINT_NAME.to_string(),
            args: Vec::new(),
            ret: CLType::Unit,
            access: EntryPointAccess::Public,
            entry_point_type: EntryPointType::Session,
        }
    }
}

impl ToBytes for EntryPoint {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.append(&mut self.name.to_bytes()?);
        result.append(&mut self.args.to_bytes()?);
        self.ret.append_bytes(&mut result)?;
        result.append(&mut self.access.to_bytes()?);
        result.append(&mut self.entry_point_type.to_bytes()?);

        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length()
            + self.args.serialized_length()
            + self.ret.serialized_length()
            + self.access.serialized_length()
            + self.entry_point_type.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.name().write_bytes(writer)?;
        self.args.write_bytes(writer)?;
        self.ret.append_bytes(writer)?;
        self.access().write_bytes(writer)?;
        self.entry_point_type().write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for EntryPoint {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, bytes) = String::from_bytes(bytes)?;
        let (args, bytes) = Vec::<Parameter>::from_bytes(bytes)?;
        let (ret, bytes) = CLType::from_bytes(bytes)?;
        let (access, bytes) = EntryPointAccess::from_bytes(bytes)?;
        let (entry_point_type, bytes) = EntryPointType::from_bytes(bytes)?;

        Ok((
            EntryPoint {
                name,
                args,
                ret,
                access,
                entry_point_type,
            },
            bytes,
        ))
    }
}

/// Enum describing the possible access control options for a contract entry
/// point (method).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntryPointAccess {
    /// Anyone can call this method (no access controls).
    Public,
    /// Only users from the listed groups may call this method. Note: if the
    /// list is empty then this method is not callable from outside the
    /// contract.
    Groups(Vec<Group>),
}

const ENTRYPOINTACCESS_PUBLIC_TAG: u8 = 1;
const ENTRYPOINTACCESS_GROUPS_TAG: u8 = 2;

impl EntryPointAccess {
    /// Constructor for access granted to only listed groups.
    pub fn groups(labels: &[&str]) -> Self {
        let list: Vec<Group> = labels.iter().map(|s| Group(String::from(*s))).collect();
        EntryPointAccess::Groups(list)
    }
}

impl ToBytes for EntryPointAccess {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;

        match self {
            EntryPointAccess::Public => {
                result.push(ENTRYPOINTACCESS_PUBLIC_TAG);
            }
            EntryPointAccess::Groups(groups) => {
                result.push(ENTRYPOINTACCESS_GROUPS_TAG);
                result.append(&mut groups.to_bytes()?);
            }
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        match self {
            EntryPointAccess::Public => 1,
            EntryPointAccess::Groups(groups) => 1 + groups.serialized_length(),
        }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            EntryPointAccess::Public => {
                writer.push(ENTRYPOINTACCESS_PUBLIC_TAG);
            }
            EntryPointAccess::Groups(groups) => {
                writer.push(ENTRYPOINTACCESS_GROUPS_TAG);
                groups.write_bytes(writer)?;
            }
        }
        Ok(())
    }
}

impl FromBytes for EntryPointAccess {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, bytes) = u8::from_bytes(bytes)?;

        match tag {
            ENTRYPOINTACCESS_PUBLIC_TAG => Ok((EntryPointAccess::Public, bytes)),
            ENTRYPOINTACCESS_GROUPS_TAG => {
                let (groups, bytes) = Vec::<Group>::from_bytes(bytes)?;
                let result = EntryPointAccess::Groups(groups);
                Ok((result, bytes))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Parameter to a method
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Parameter {
    name: String,
    cl_type: CLType,
}

impl Parameter {
    /// `Parameter` constructor.
    pub fn new<T: Into<String>>(name: T, cl_type: CLType) -> Self {
        Parameter {
            name: name.into(),
            cl_type,
        }
    }

    /// Get the type of this argument.
    pub fn cl_type(&self) -> &CLType {
        &self.cl_type
    }

    /// Get a reference to the parameter's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl From<Parameter> for (String, CLType) {
    fn from(parameter: Parameter) -> Self {
        (parameter.name, parameter.cl_type)
    }
}

impl ToBytes for Parameter {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = ToBytes::to_bytes(&self.name)?;
        self.cl_type.append_bytes(&mut result)?;

        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.name) + self.cl_type.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.name.write_bytes(writer)?;
        self.cl_type.append_bytes(writer)
    }
}

impl FromBytes for Parameter {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, bytes) = String::from_bytes(bytes)?;
        let (cl_type, bytes) = CLType::from_bytes(bytes)?;

        Ok((Parameter { name, cl_type }, bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessRights, URef, UREF_ADDR_LENGTH};
    use alloc::borrow::ToOwned;

    fn make_contract_package() -> Package {
        let mut contract_package = Package::new(
            URef::new([0; 32], AccessRights::NONE),
            ContractVersions::default(),
            DisabledVersions::default(),
            Groups::default(),
            ContractPackageStatus::default(),
            ContractPackageKind::Wasm,
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
        let contract_hash = [42; 32];
        let _contract_wasm_hash = [43; 32];
        let _named_keys = NamedKeys::new();
        let protocol_version = ProtocolVersion::V1_0_0;

        contract_package
            .insert_contract_version(protocol_version.value().major, contract_hash.into());

        contract_package
    }

    #[test]
    fn next_contract_version() {
        let major = 1;
        let mut contract_package = Package::new(
            URef::new([0; 32], AccessRights::NONE),
            ContractVersions::default(),
            DisabledVersions::default(),
            Groups::default(),
            ContractPackageStatus::default(),
            ContractPackageKind::Wasm,
        );
        assert_eq!(contract_package.next_contract_version_for(major), 1);

        let next_version = contract_package.insert_contract_version(major, [123; 32].into());
        assert_eq!(next_version, ContractVersionKey::new(major, 1));
        assert_eq!(contract_package.next_contract_version_for(major), 2);
        let next_version_2 = contract_package.insert_contract_version(major, [124; 32].into());
        assert_eq!(next_version_2, ContractVersionKey::new(major, 2));

        let major = 2;
        assert_eq!(contract_package.next_contract_version_for(major), 1);
        let next_version_3 = contract_package.insert_contract_version(major, [42; 32].into());
        assert_eq!(next_version_3, ContractVersionKey::new(major, 1));
    }

    #[test]
    fn roundtrip_serialization() {
        let contract_package = make_contract_package();
        let bytes = contract_package.to_bytes().expect("should serialize");
        let (decoded_package, rem) = Package::from_bytes(&bytes).expect("should deserialize");
        assert_eq!(contract_package, decoded_package);
        assert_eq!(rem.len(), 0);
    }

    #[test]
    fn should_remove_group() {
        let mut contract_package = make_contract_package();

        assert!(!contract_package.remove_group(&Group::new("Non-existent group")));
        assert!(contract_package.remove_group(&Group::new("Group 1")));
        assert!(!contract_package.remove_group(&Group::new("Group 1"))); // Group no longer exists
    }

    #[test]
    fn should_disable_contract_version() {
        const CONTRACT_HASH: ContractHash = ContractHash::new([123; 32]);
        let mut contract_package = make_contract_package();

        assert!(
            !contract_package.is_contract_enabled(&CONTRACT_HASH),
            "nonexisting contract contract should return false"
        );

        assert_eq!(
            contract_package.disable_contract_version(CONTRACT_HASH),
            Err(Error::ContractNotFound),
            "should return contract not found error"
        );

        assert!(
            !contract_package.is_contract_enabled(&CONTRACT_HASH),
            "disabling missing contract shouldnt change outcome"
        );

        let next_version = contract_package.insert_contract_version(1, CONTRACT_HASH);
        assert!(
            contract_package.is_version_enabled(next_version),
            "version should exist and be enabled"
        );

        assert!(
            contract_package.is_contract_enabled(&CONTRACT_HASH),
            "contract should be enabled"
        );

        assert_eq!(
            contract_package.disable_contract_version(CONTRACT_HASH),
            Ok(()),
            "should be able to disable version"
        );

        assert!(
            !contract_package.is_contract_enabled(&CONTRACT_HASH),
            "contract should be disabled"
        );

        assert_eq!(
            contract_package.lookup_contract_hash(next_version),
            None,
            "should not return disabled contract version"
        );

        assert!(
            !contract_package.is_version_enabled(next_version),
            "version should not be enabled"
        );
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

    #[test]
    fn should_extract_access_rights() {
        const MAIN_PURSE: URef = URef::new([2; 32], AccessRights::READ_ADD_WRITE);

        let contract_hash = ContractHash([255; 32]);
        let uref = URef::new([84; UREF_ADDR_LENGTH], AccessRights::READ_ADD);
        let uref_r = URef::new([42; UREF_ADDR_LENGTH], AccessRights::READ);
        let uref_a = URef::new([42; UREF_ADDR_LENGTH], AccessRights::ADD);
        let uref_w = URef::new([42; UREF_ADDR_LENGTH], AccessRights::WRITE);
        let mut named_keys = NamedKeys::new();
        named_keys.insert("a".to_string(), Key::URef(uref_r));
        named_keys.insert("b".to_string(), Key::URef(uref_a));
        named_keys.insert("c".to_string(), Key::URef(uref_w));
        named_keys.insert("d".to_string(), Key::URef(uref));
        let associated_keys = AssociatedKeys::new(AccountHash::new([254; 32]), Weight::new(1));
        let contract = AddressableEntity::new(
            ContractPackageHash::new([254; 32]),
            ContractWasmHash::new([253; 32]),
            named_keys,
            EntryPoints::default(),
            ProtocolVersion::V1_0_0,
            MAIN_PURSE,
            associated_keys,
            ActionThresholds::new(Weight::new(1), Weight::new(1))
                .expect("should create thresholds"),
        );
        let access_rights = contract.extract_access_rights(contract_hash);
        let expected_uref = URef::new([42; UREF_ADDR_LENGTH], AccessRights::READ_ADD_WRITE);
        assert!(
            access_rights.has_access_rights_to_uref(&uref),
            "urefs in named keys should be included in access rights"
        );
        assert!(
            access_rights.has_access_rights_to_uref(&expected_uref),
            "multiple access right bits to the same uref should coalesce"
        );
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        // #![proptest_config(ProptestConfig {
        //     cases: 1024,
        //     .. ProptestConfig::default()
        // })]

        #[test]
        fn test_value_contract(contract in gens::addressable_entity_arb()) {
            bytesrepr::test_serialization_roundtrip(&contract);
        }

        #[test]
        fn test_value_contract_package(contract_pkg in gens::contract_package_arb()) {
            bytesrepr::test_serialization_roundtrip(&contract_pkg);
        }
    }
}
