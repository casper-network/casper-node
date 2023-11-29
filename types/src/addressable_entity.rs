//! Data types for supporting contract headers feature.
// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

pub mod action_thresholds;
mod action_type;
pub mod associated_keys;
mod error;
mod named_keys;
mod weight;

use alloc::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec::Vec,
};
use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use core::{
    array::TryFromSliceError,
    convert::{TryFrom, TryInto},
    fmt::{self, Debug, Display, Formatter},
    iter,
};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "json-schema")]
use serde_map_to_array::KeyValueJsonSchema;
use serde_map_to_array::{BTreeMapToArray, KeyValueLabels};

pub use self::{
    action_thresholds::ActionThresholds,
    action_type::ActionType,
    associated_keys::AssociatedKeys,
    error::{
        FromAccountHashStrError, SetThresholdFailure, TryFromIntError,
        TryFromSliceForAccountHashError,
    },
    named_keys::NamedKeys,
    weight::{Weight, WEIGHT_SERIALIZED_LENGTH},
};

use crate::{
    account::{Account, AccountHash},
    byte_code::ByteCodeHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex,
    contract_messages::TopicNameHash,
    contracts::{Contract, ContractHash},
    system::SystemEntityType,
    uref::{self, URef},
    AccessRights, ApiError, CLType, CLTyped, CLValue, CLValueError, ContextAccessRights, Group,
    HashAddr, Key, KeyTag, PackageHash, ProtocolVersion, PublicKey, Tagged, BLAKE2B_DIGEST_LENGTH,
    KEY_HASH_LENGTH,
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

const ADDRESSABLE_ENTITY_STRING_PREFIX: &str = "addressable-entity-";

const ENTITY_PREFIX: &str = "addressable-entity-";
const ACCOUNT_ENTITY_PREFIX: &str = "account-";
const CONTRACT_ENTITY_PREFIX: &str = "contract-";
const SYSTEM_ENTITY_PREFIX: &str = "system-";
const NAMED_KEY_PREFIX: &str = "named-key-";

/// Set of errors which may happen when working with contract headers.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
#[non_exhaustive]
pub enum Error {
    /// Attempt to override an existing or previously existing version with a
    /// new header (this is not allowed to ensure immutability of a given
    /// version).
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(1, Error::PreviouslyUsedVersion as u8);
    /// ```
    PreviouslyUsedVersion = 1,
    /// Attempted to disable a contract that does not exist.
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(2, Error::EntityNotFound as u8);
    /// ```
    EntityNotFound = 2,
    /// Attempted to create a user group which already exists (use the update
    /// function to change an existing user group).
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(3, Error::GroupAlreadyExists as u8);
    /// ```
    GroupAlreadyExists = 3,
    /// Attempted to add a new user group which exceeds the allowed maximum
    /// number of groups.
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(4, Error::MaxGroupsExceeded as u8);
    /// ```
    MaxGroupsExceeded = 4,
    /// Attempted to add a new URef to a group, which resulted in the total
    /// number of URefs across all user groups to exceed the allowed maximum.
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(5, Error::MaxTotalURefsExceeded as u8);
    /// ```
    MaxTotalURefsExceeded = 5,
    /// Attempted to remove a URef from a group, which does not exist in the
    /// group.
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(6, Error::GroupDoesNotExist as u8);
    /// ```
    GroupDoesNotExist = 6,
    /// Attempted to remove unknown URef from the group.
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(7, Error::UnableToRemoveURef as u8);
    /// ```
    UnableToRemoveURef = 7,
    /// Group is use by at least one active contract.
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(8, Error::GroupInUse as u8);
    /// ```
    GroupInUse = 8,
    /// URef already exists in given group.
    /// ```
    /// # use casper_types::addressable_entity::Error;
    /// assert_eq!(9, Error::URefAlreadyExists as u8);
    /// ```
    URefAlreadyExists = 9,
}

impl TryFrom<u8> for Error {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let error = match value {
            v if v == Self::PreviouslyUsedVersion as u8 => Self::PreviouslyUsedVersion,
            v if v == Self::EntityNotFound as u8 => Self::EntityNotFound,
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
    /// Error parsing from bytes.
    BytesRepr(bytesrepr::Error),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

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
            FromStrError::Hash(error) => write!(f, "hash from string error: {}", error),
            FromStrError::URef(error) => write!(f, "uref from string error: {:?}", error),
            FromStrError::Account(error) => {
                write!(f, "account hash from string error: {:?}", error)
            }
            FromStrError::BytesRepr(error) => {
                write!(f, "bytesrepr error: {:?}", error)
            }
        }
    }
}

/// A newtype wrapping a `HashAddr` which references an [`AddressableEntity`] in the global state.
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The hex-encoded address of the addressable entity.")
)]
pub struct AddressableEntityHash(
    #[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))] HashAddr,
);

impl AddressableEntityHash {
    /// Constructs a new `AddressableEntityHash` from the raw bytes of the contract hash.
    pub const fn new(value: HashAddr) -> AddressableEntityHash {
        AddressableEntityHash(value)
    }

    /// Returns the raw bytes of the contract hash as an array.
    pub fn value(&self) -> HashAddr {
        self.0
    }

    /// Returns the raw bytes of the contract hash as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `AddressableEntityHash` for users getting and putting.
    pub fn to_formatted_string(self) -> String {
        format!(
            "{}{}",
            ADDRESSABLE_ENTITY_STRING_PREFIX,
            base16::encode_lower(&self.0),
        )
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a
    /// `AddressableEntityHash`.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(ADDRESSABLE_ENTITY_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = HashAddr::try_from(checksummed_hex::decode(remainder)?.as_ref())?;
        Ok(AddressableEntityHash(bytes))
    }
}

impl From<ContractHash> for AddressableEntityHash {
    fn from(contract_hash: ContractHash) -> Self {
        AddressableEntityHash::new(contract_hash.value())
    }
}

impl Display for AddressableEntityHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for AddressableEntityHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(
            f,
            "AddressableEntityHash({})",
            base16::encode_lower(&self.0)
        )
    }
}

impl CLTyped for AddressableEntityHash {
    fn cl_type() -> CLType {
        CLType::ByteArray(KEY_HASH_LENGTH as u32)
    }
}

impl ToBytes for AddressableEntityHash {
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

impl FromBytes for AddressableEntityHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((AddressableEntityHash::new(bytes), rem))
    }
}

impl From<[u8; 32]> for AddressableEntityHash {
    fn from(bytes: [u8; 32]) -> Self {
        AddressableEntityHash(bytes)
    }
}

impl TryFrom<Key> for AddressableEntityHash {
    type Error = ApiError;

    fn try_from(value: Key) -> Result<Self, Self::Error> {
        if let Key::AddressableEntity(entity_addr) = value {
            Ok(AddressableEntityHash::new(entity_addr.value()))
        } else {
            Err(ApiError::Formatting)
        }
    }
}

impl Serialize for AddressableEntityHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for AddressableEntityHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            AddressableEntityHash::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = HashAddr::deserialize(deserializer)?;
            Ok(AddressableEntityHash(bytes))
        }
    }
}

impl AsRef<[u8]> for AddressableEntityHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TryFrom<&[u8]> for AddressableEntityHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForContractHashError> {
        HashAddr::try_from(bytes)
            .map(AddressableEntityHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

impl TryFrom<&Vec<u8>> for AddressableEntityHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        HashAddr::try_from(bytes as &[u8])
            .map(AddressableEntityHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<AddressableEntityHash> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AddressableEntityHash {
        AddressableEntityHash(rng.gen())
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

/// Collection of named entry points.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(transparent, deny_unknown_fields)]
pub struct EntryPoints(
    #[serde(with = "BTreeMapToArray::<String, EntryPoint, EntryPointLabels>")]
    BTreeMap<String, EntryPoint>,
);

impl ToBytes for EntryPoints {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }
}

impl FromBytes for EntryPoints {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (entry_points_map, remainder) = BTreeMap::<String, EntryPoint>::from_bytes(bytes)?;
        Ok((EntryPoints(entry_points_map), remainder))
    }
}

impl Default for EntryPoints {
    fn default() -> Self {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::default();
        entry_points.add_entry_point(entry_point);
        entry_points
    }
}

impl EntryPoints {
    /// Constructs a new, empty `EntryPoints`.
    pub const fn new() -> EntryPoints {
        EntryPoints(BTreeMap::<String, EntryPoint>::new())
    }

    /// Constructs a new `EntryPoints` with a single entry for the default `EntryPoint`.
    pub fn new_with_default_entry_point() -> Self {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::default();
        entry_points.add_entry_point(entry_point);
        entry_points
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

    /// Checks if any of the entry points are of the type Session.
    pub fn contains_stored_session(&self) -> bool {
        self.0
            .values()
            .any(|entry_point| entry_point.entry_point_type == EntryPointType::Session)
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

struct EntryPointLabels;

impl KeyValueLabels for EntryPointLabels {
    const KEY: &'static str = "name";
    const VALUE: &'static str = "entry_point";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for EntryPointLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("NamedEntryPoint");
}

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[repr(u8)]
pub enum EntityKindTag {
    System = 0,
    Account = 1,
    SmartContract = 2,
}

impl TryFrom<u8> for EntityKindTag {
    type Error = bytesrepr::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(EntityKindTag::System),
            1 => Ok(EntityKindTag::Account),
            2 => Ok(EntityKindTag::SmartContract),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl ToBytes for EntityKindTag {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        (*self as u8).to_bytes()
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        (*self as u8).write_bytes(writer)
    }
}

impl FromBytes for EntityKindTag {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (entity_kind_tag, remainder) = u8::from_bytes(bytes)?;
        Ok((entity_kind_tag.try_into()?, remainder))
    }
}

impl Display for EntityKindTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EntityKindTag::System => {
                write!(f, "system")
            }
            EntityKindTag::Account => {
                write!(f, "account")
            }
            EntityKindTag::SmartContract => {
                write!(f, "contract")
            }
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<EntityKindTag> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EntityKindTag {
        match rng.gen_range(0..=2) {
            0 => EntityKindTag::System,
            1 => EntityKindTag::Account,
            2 => EntityKindTag::SmartContract,
            _ => unreachable!(),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
/// The type of Package.
pub enum EntityKind {
    /// Package associated with a native contract implementation.
    System(SystemEntityType),
    /// Package associated with an Account hash.
    Account(AccountHash),
    /// Packages associated with Wasm stored on chain.
    #[default]
    SmartContract,
}

impl EntityKind {
    /// Returns the Account hash associated with a Package based on the package kind.
    pub fn maybe_account_hash(&self) -> Option<AccountHash> {
        match self {
            Self::Account(account_hash) => Some(*account_hash),
            Self::SmartContract | Self::System(_) => None,
        }
    }

    /// Returns the associated key set based on the Account hash set in the package kind.
    pub fn associated_keys(&self) -> AssociatedKeys {
        match self {
            Self::Account(account_hash) => AssociatedKeys::new(*account_hash, Weight::new(1)),
            Self::SmartContract | Self::System(_) => AssociatedKeys::default(),
        }
    }

    /// Returns if the current package is either a system contract or the system entity.
    pub fn is_system(&self) -> bool {
        matches!(self, Self::System(_))
    }

    /// Returns if the current package is the system mint.
    pub fn is_system_mint(&self) -> bool {
        matches!(self, Self::System(SystemEntityType::Mint))
    }

    /// Returns if the current package is the system auction.
    pub fn is_system_auction(&self) -> bool {
        matches!(self, Self::System(SystemEntityType::Auction))
    }

    /// Returns if the current package is associated with the system addressable entity.
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

impl Tagged<EntityKindTag> for EntityKind {
    fn tag(&self) -> EntityKindTag {
        match self {
            EntityKind::System(_) => EntityKindTag::System,
            EntityKind::Account(_) => EntityKindTag::Account,
            EntityKind::SmartContract => EntityKindTag::SmartContract,
        }
    }
}

impl Tagged<u8> for EntityKind {
    fn tag(&self) -> u8 {
        let package_kind_tag: EntityKindTag = self.tag();
        package_kind_tag as u8
    }
}

impl ToBytes for EntityKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                EntityKind::SmartContract => 0,
                EntityKind::System(system_entity_type) => system_entity_type.serialized_length(),
                EntityKind::Account(account_hash) => account_hash.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            EntityKind::SmartContract => {
                writer.push(self.tag());
                Ok(())
            }
            EntityKind::System(system_entity_type) => {
                writer.push(self.tag());
                system_entity_type.write_bytes(writer)
            }
            EntityKind::Account(account_hash) => {
                writer.push(self.tag());
                account_hash.write_bytes(writer)
            }
        }
    }
}

impl FromBytes for EntityKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = EntityKindTag::from_bytes(bytes)?;
        match tag {
            EntityKindTag::System => {
                let (entity_type, remainder) = SystemEntityType::from_bytes(remainder)?;
                Ok((EntityKind::System(entity_type), remainder))
            }
            EntityKindTag::Account => {
                let (account_hash, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((EntityKind::Account(account_hash), remainder))
            }
            EntityKindTag::SmartContract => Ok((EntityKind::SmartContract, remainder)),
        }
    }
}

impl Display for EntityKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EntityKind::SmartContract => {
                write!(f, "PackageKind:Wasm")
            }
            EntityKind::System(system_entity) => {
                write!(f, "PackageKind:System({})", system_entity)
            }
            EntityKind::Account(account_hash) => {
                write!(f, "PackageKind:Account({})", account_hash)
            }
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<EntityKind> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EntityKind {
        match rng.gen_range(0..=2) {
            0 => EntityKind::System(rng.gen()),
            1 => EntityKind::Account(rng.gen()),
            2 => EntityKind::SmartContract,
            _ => unreachable!(),
        }
    }
}

/// The address for an AddressableEntity which contains the 32 bytes and tagging information.
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntityAddr {
    /// The address for a system entity account or contract.
    System(HashAddr),
    /// The address of an entity that corresponds to an Account.
    Account(HashAddr),
    /// The address of an entity that corresponds to a Userland smart contract.
    SmartContract(HashAddr),
}

impl EntityAddr {
    /// The length in bytes of a [`EntityAddr`].
    pub const ENTITY_ADDR_LENGTH: usize = U8_SERIALIZED_LENGTH + KEY_HASH_LENGTH;

    /// Constructs a new [`EntityAddr`] for a system entity.
    pub const fn new_system_entity_addr(hash_addr: [u8; KEY_HASH_LENGTH]) -> Self {
        Self::System(hash_addr)
    }

    /// Constructs a new [`EntityAddr`] for an Account entity.
    pub const fn new_account_entity_addr(hash_addr: [u8; KEY_HASH_LENGTH]) -> Self {
        Self::Account(hash_addr)
    }

    /// Constructs a new [`EntityAddr`] for a smart contract.
    pub const fn new_contract_entity_addr(hash_addr: [u8; KEY_HASH_LENGTH]) -> Self {
        Self::SmartContract(hash_addr)
    }

    /// Constructs a new [`EntityAddr`] based on the supplied tag.
    pub fn new_with_tag(entity_kind: EntityKind, hash_addr: [u8; KEY_HASH_LENGTH]) -> Self {
        match entity_kind {
            EntityKind::System(_) => Self::new_system_entity_addr(hash_addr),
            EntityKind::Account(_) => Self::new_account_entity_addr(hash_addr),
            EntityKind::SmartContract => Self::new_contract_entity_addr(hash_addr),
        }
    }

    /// Returns the tag of the [`EntityAddr`].
    pub fn tag(&self) -> EntityKindTag {
        match self {
            EntityAddr::System(_) => EntityKindTag::System,
            EntityAddr::Account(_) => EntityKindTag::Account,
            EntityAddr::SmartContract(_) => EntityKindTag::SmartContract,
        }
    }

    /// Returns the 32 bytes of the [`EntityAddr`].
    pub fn value(&self) -> HashAddr {
        match self {
            EntityAddr::System(hash_addr)
            | EntityAddr::Account(hash_addr)
            | EntityAddr::SmartContract(hash_addr) => *hash_addr,
        }
    }

    /// Returns the common prefix of all NamedKey entries.
    pub fn named_keys_prefix(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = Vec::with_capacity(self.serialized_length() + 1);
        ret.push(KeyTag::NamedKey as u8);
        self.write_bytes(&mut ret)?;
        Ok(ret)
    }

    /// Returns the formatted String representation of the [`EntityAddr`].
    pub fn to_formatted_string(&self) -> String {
        format!("{}", self)
    }

    /// Constructs an [`EntityAddr`] from a formatted String.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        if let Some(entity) = input.strip_prefix(ENTITY_PREFIX) {
            let (addr_str, tag) = if let Some(str) = entity.strip_prefix(ACCOUNT_ENTITY_PREFIX) {
                (str, EntityKindTag::Account)
            } else if let Some(str) = entity.strip_prefix(SYSTEM_ENTITY_PREFIX) {
                (str, EntityKindTag::System)
            } else if let Some(str) = entity.strip_prefix(CONTRACT_ENTITY_PREFIX) {
                (str, EntityKindTag::SmartContract)
            } else {
                return Err(FromStrError::InvalidPrefix);
            };
            let addr = checksummed_hex::decode(addr_str).map_err(FromStrError::Hex)?;
            let hash_addr = HashAddr::try_from(addr.as_ref()).map_err(FromStrError::Hash)?;
            let entity_addr = match tag {
                EntityKindTag::System => EntityAddr::new_system_entity_addr(hash_addr),
                EntityKindTag::Account => EntityAddr::new_account_entity_addr(hash_addr),
                EntityKindTag::SmartContract => EntityAddr::new_contract_entity_addr(hash_addr),
            };

            return Ok(entity_addr);
        }

        Err(FromStrError::InvalidPrefix)
    }
}

impl ToBytes for EntityAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.push(self.tag() as u8);
        buffer.append(&mut self.value().to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        EntityAddr::ENTITY_ADDR_LENGTH
    }
}

impl FromBytes for EntityAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (EntityKindTag, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            EntityKindTag::System => {
                HashAddr::from_bytes(remainder).map(|(hash_addr, remainder)| {
                    (EntityAddr::new_system_entity_addr(hash_addr), remainder)
                })
            }
            EntityKindTag::Account => {
                HashAddr::from_bytes(remainder).map(|(hash_addr, remainder)| {
                    (EntityAddr::new_account_entity_addr(hash_addr), remainder)
                })
            }
            EntityKindTag::SmartContract => {
                HashAddr::from_bytes(remainder).map(|(hash_addr, remainder)| {
                    (EntityAddr::new_contract_entity_addr(hash_addr), remainder)
                })
            }
        }
    }
}

impl From<EntityAddr> for AddressableEntityHash {
    fn from(entity_addr: EntityAddr) -> Self {
        AddressableEntityHash::new(entity_addr.value())
    }
}

impl Display for EntityAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}-{}",
            ADDRESSABLE_ENTITY_STRING_PREFIX,
            self.tag(),
            base16::encode_lower(&self.value())
        )
    }
}

impl Debug for EntityAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EntityAddr::System(hash_addr) => {
                write!(f, "EntityAddr::System({:?})", hash_addr)
            }
            EntityAddr::Account(hash_addr) => {
                write!(f, "EntityAddr::Account({:?})", hash_addr)
            }
            EntityAddr::SmartContract(hash_addr) => {
                write!(f, "EntityAddr::SmartContract({:?})", hash_addr)
            }
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<EntityAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EntityAddr {
        match rng.gen_range(0..=2) {
            0 => EntityAddr::System(rng.gen()),
            1 => EntityAddr::Account(rng.gen()),
            2 => EntityAddr::SmartContract(rng.gen()),
            _ => unreachable!(),
        }
    }
}

/// A NamedKey address.

#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct NamedKeyAddr {
    /// The address of the entity.
    base_addr: EntityAddr,
    /// The bytes of the name.
    string_bytes: [u8; KEY_HASH_LENGTH],
}

impl NamedKeyAddr {
    /// The length in bytes of a [`NamedKeyAddr`].
    pub const NAMED_KEY_ADDR_BASE_LENGTH: usize = 1 + EntityAddr::ENTITY_ADDR_LENGTH;

    /// Constructs a new [`NamedKeyAddr`] based on the supplied bytes.
    pub const fn new_named_key_entry(
        entity_addr: EntityAddr,
        string_bytes: [u8; KEY_HASH_LENGTH],
    ) -> Self {
        Self {
            base_addr: entity_addr,
            string_bytes,
        }
    }

    /// Constructs a new [`NamedKeyAddr`] based on string name.
    /// Will fail if the string cannot be serialized.
    pub fn new_from_string(
        entity_addr: EntityAddr,
        entry: String,
    ) -> Result<Self, bytesrepr::Error> {
        let bytes = entry.to_bytes()?;
        let mut hasher = VarBlake2b::new(BLAKE2B_DIGEST_LENGTH).expect("should create hasher");
        hasher.update(bytes);
        // NOTE: Assumed safe as size of `HashAddr` equals to the output provided by hasher.
        let mut string_bytes = HashAddr::default();
        hasher.finalize_variable(|hash| string_bytes.clone_from_slice(hash));
        Ok(Self::new_named_key_entry(entity_addr, string_bytes))
    }

    /// Returns the encapsulated [`EntityAddr`].
    pub fn entity_addr(&self) -> EntityAddr {
        self.base_addr
    }

    /// Returns the formatted String representation of the [`NamedKeyAddr`].
    pub fn to_formatted_string(&self) -> String {
        format!("{}", self)
    }

    /// Constructs a [`NamedKeyAddr`] from a formatted string.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        if let Some(named_key) = input.strip_prefix(NAMED_KEY_PREFIX) {
            if let Some((entity_addr_str, string_bytes_str)) = named_key.rsplit_once('-') {
                let entity_addr = EntityAddr::from_formatted_str(entity_addr_str)?;
                let string_bytes =
                    checksummed_hex::decode(string_bytes_str).map_err(FromStrError::Hex)?;
                let (string_bytes, _) =
                    FromBytes::from_vec(string_bytes).map_err(FromStrError::BytesRepr)?;
                return Ok(Self::new_named_key_entry(entity_addr, string_bytes));
            };
        }

        Err(FromStrError::InvalidPrefix)
    }
}

impl ToBytes for NamedKeyAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.base_addr.to_bytes()?);
        buffer.append(&mut self.string_bytes.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.base_addr.serialized_length() + self.string_bytes.serialized_length()
    }
}

impl FromBytes for NamedKeyAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (base_addr, remainder) = EntityAddr::from_bytes(bytes)?;
        let (string_bytes, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            Self {
                base_addr,
                string_bytes,
            },
            remainder,
        ))
    }
}

impl Display for NamedKeyAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}-{}",
            NAMED_KEY_PREFIX,
            self.base_addr,
            base16::encode_lower(&self.string_bytes)
        )
    }
}

impl Debug for NamedKeyAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "NamedKeyAddr({:?}-{:?})",
            self.base_addr,
            base16::encode_lower(&self.string_bytes)
        )
    }
}

/// A NamedKey value.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct NamedKeyValue {
    /// The actual `Key` encoded as a CLValue.
    named_key: CLValue,
    /// The name of the `Key` encoded as a CLValue.
    name: CLValue,
}

impl NamedKeyValue {
    /// Constructs a new [`NamedKeyValue`].
    pub fn new(key: CLValue, name: CLValue) -> Self {
        Self {
            named_key: key,
            name,
        }
    }

    /// Constructs a new [`NamedKeyValue`] from its [`Key`] and [`String`].
    pub fn from_concrete_values(named_key: Key, name: String) -> Result<Self, CLValueError> {
        let key_cl_value = CLValue::from_t(named_key)?;
        let string_cl_value = CLValue::from_t(name)?;
        Ok(Self::new(key_cl_value, string_cl_value))
    }

    /// Returns the [`Key`] as a CLValue.
    pub fn get_key_as_cl_value(&self) -> &CLValue {
        &self.named_key
    }

    /// Returns the [`String`] as a CLValue.
    pub fn get_name_as_cl_value(&self) -> &CLValue {
        &self.name
    }

    /// Returns the concrete `Key` value
    pub fn get_key(&self) -> Result<Key, CLValueError> {
        self.named_key.clone().into_t::<Key>()
    }

    /// Returns the concrete `String` value
    pub fn get_name(&self) -> Result<String, CLValueError> {
        self.name.clone().into_t::<String>()
    }
}

impl ToBytes for NamedKeyValue {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.named_key.to_bytes()?);
        buffer.append(&mut self.name.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.named_key.serialized_length() + self.name.serialized_length()
    }
}

impl FromBytes for NamedKeyValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (named_key, remainder) = CLValue::from_bytes(bytes)?;
        let (name, remainder) = CLValue::from_bytes(remainder)?;
        Ok((Self { named_key, name }, remainder))
    }
}

/// Collection of named message topics.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(transparent, deny_unknown_fields)]
pub struct MessageTopics(
    #[serde(with = "BTreeMapToArray::<String, TopicNameHash, MessageTopicLabels>")]
    BTreeMap<String, TopicNameHash>,
);

impl ToBytes for MessageTopics {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }
}

impl FromBytes for MessageTopics {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (message_topics_map, remainder) = BTreeMap::<String, TopicNameHash>::from_bytes(bytes)?;
        Ok((MessageTopics(message_topics_map), remainder))
    }
}

impl MessageTopics {
    /// Adds new message topic by topic name.
    pub fn add_topic(
        &mut self,
        topic_name: &str,
        topic_name_hash: TopicNameHash,
    ) -> Result<(), MessageTopicError> {
        if self.0.len() >= u32::MAX as usize {
            return Err(MessageTopicError::MaxTopicsExceeded);
        }

        match self.0.entry(topic_name.to_string()) {
            Entry::Vacant(entry) => {
                entry.insert(topic_name_hash);
                Ok(())
            }
            Entry::Occupied(_) => Err(MessageTopicError::DuplicateTopic),
        }
    }

    /// Checks if given topic name exists.
    pub fn has_topic(&self, topic_name: &str) -> bool {
        self.0.contains_key(topic_name)
    }

    /// Gets the topic hash from the collection by its topic name.
    pub fn get(&self, topic_name: &str) -> Option<&TopicNameHash> {
        self.0.get(topic_name)
    }

    /// Returns the length of the message topics.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if no message topics are registered.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an iterator over the topic name and its hash.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &TopicNameHash)> {
        self.0.iter()
    }
}

struct MessageTopicLabels;

impl KeyValueLabels for MessageTopicLabels {
    const KEY: &'static str = "topic_name";
    const VALUE: &'static str = "topic_name_hash";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for MessageTopicLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("MessageTopic");
}

impl From<BTreeMap<String, TopicNameHash>> for MessageTopics {
    fn from(topics: BTreeMap<String, TopicNameHash>) -> MessageTopics {
        MessageTopics(topics)
    }
}

/// Errors that can occur while adding a new topic.
#[derive(PartialEq, Eq, Debug, Clone)]
#[non_exhaustive]
pub enum MessageTopicError {
    /// Topic already exists.
    DuplicateTopic,
    /// Maximum number of topics exceeded.
    MaxTopicsExceeded,
    /// Topic name size exceeded.
    TopicNameSizeExceeded,
}

/// Methods and type signatures supported by a contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct AddressableEntity {
    protocol_version: ProtocolVersion,
    entity_kind: EntityKind,
    package_hash: PackageHash,
    byte_code_hash: ByteCodeHash,
    main_purse: URef,

    entry_points: EntryPoints,
    associated_keys: AssociatedKeys,
    action_thresholds: ActionThresholds,
    message_topics: MessageTopics,
}

impl From<AddressableEntity>
    for (
        PackageHash,
        ByteCodeHash,
        EntryPoints,
        ProtocolVersion,
        URef,
        AssociatedKeys,
        ActionThresholds,
    )
{
    fn from(entity: AddressableEntity) -> Self {
        (
            entity.package_hash,
            entity.byte_code_hash,
            entity.entry_points,
            entity.protocol_version,
            entity.main_purse,
            entity.associated_keys,
            entity.action_thresholds,
        )
    }
}

impl AddressableEntity {
    /// `AddressableEntity` constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        package_hash: PackageHash,
        byte_code_hash: ByteCodeHash,
        entry_points: EntryPoints,
        protocol_version: ProtocolVersion,
        main_purse: URef,
        associated_keys: AssociatedKeys,
        action_thresholds: ActionThresholds,
        message_topics: MessageTopics,
        entity_kind: EntityKind,
    ) -> Self {
        AddressableEntity {
            package_hash,
            byte_code_hash,
            entry_points,
            protocol_version,
            main_purse,
            action_thresholds,
            associated_keys,
            message_topics,
            entity_kind,
        }
    }

    /// Hash for accessing contract package
    pub fn package_hash(&self) -> PackageHash {
        self.package_hash
    }

    /// Hash for accessing contract WASM
    pub fn byte_code_hash(&self) -> ByteCodeHash {
        self.byte_code_hash
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

    /// Adds an associated key to an addressable entity.
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

    /// Removes an associated key from an addressable entity.
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

    /// Sets new action threshold for a given action type for the addressable entity.
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

    /// Sets a new action threshold for a given action type for the account without checking against
    /// the total weight of the associated keys.
    ///
    /// This should only be called when authorized by an administrator account.
    ///
    /// Returns an error if setting the action would cause the `ActionType::Deployment` threshold to
    /// be greater than any of the other action types.
    pub fn set_action_threshold_unchecked(
        &mut self,
        action_type: ActionType,
        threshold: Weight,
    ) -> Result<(), SetThresholdFailure> {
        self.action_thresholds.set_threshold(action_type, threshold)
    }

    /// Verifies if user can set action threshold.
    pub fn can_set_threshold(&self, new_threshold: Weight) -> Result<(), SetThresholdFailure> {
        let total_weight = self.associated_keys.total_keys_weight();
        if new_threshold > total_weight {
            return Err(SetThresholdFailure::InsufficientTotalWeight);
        }
        Ok(())
    }

    /// Checks whether all authorization keys are associated with this addressable entity.
    pub fn can_authorize(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        !authorization_keys.is_empty()
            && authorization_keys
                .iter()
                .any(|e| self.associated_keys.contains_key(e))
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

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to upgrade management threshold.
    pub fn can_upgrade_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        total_weight >= *self.action_thresholds().upgrade_management()
    }

    /// Adds new entry point
    pub fn add_entry_point<T: Into<String>>(&mut self, entry_point: EntryPoint) {
        self.entry_points.add_entry_point(entry_point);
    }

    /// Addr for accessing wasm bytes
    pub fn byte_code_addr(&self) -> HashAddr {
        self.byte_code_hash.value()
    }

    /// Returns immutable reference to methods
    pub fn entry_points(&self) -> &EntryPoints {
        &self.entry_points
    }

    /// Returns a reference to the message topics
    pub fn message_topics(&self) -> &MessageTopics {
        &self.message_topics
    }

    /// Adds a new message topic to the entity
    pub fn add_message_topic(
        &mut self,
        topic_name: &str,
        topic_name_hash: TopicNameHash,
    ) -> Result<(), MessageTopicError> {
        self.message_topics.add_topic(topic_name, topic_name_hash)
    }

    /// Set protocol_version.
    pub fn set_protocol_version(&mut self, protocol_version: ProtocolVersion) {
        self.protocol_version = protocol_version;
    }

    /// Determines if `AddressableEntity` is compatible with a given `ProtocolVersion`.
    pub fn is_compatible_protocol_version(&self, protocol_version: ProtocolVersion) -> bool {
        self.protocol_version.value().major == protocol_version.value().major
    }

    /// Returns the kind of Package.
    pub fn entity_kind(&self) -> EntityKind {
        self.entity_kind
    }

    /// Is the given Package associated to an Account.
    pub fn is_account_kind(&self) -> bool {
        matches!(self.entity_kind, EntityKind::Account(_))
    }

    /// Key for the addressable entity
    pub fn entity_key(&self, entity_hash: AddressableEntityHash) -> Key {
        match self.entity_kind {
            EntityKind::System(_) => {
                Key::addressable_entity_key(EntityKindTag::System, entity_hash)
            }
            EntityKind::Account(_) => {
                Key::addressable_entity_key(EntityKindTag::Account, entity_hash)
            }
            EntityKind::SmartContract => {
                Key::addressable_entity_key(EntityKindTag::SmartContract, entity_hash)
            }
        }
    }

    /// Extracts the access rights from the named keys of the addressable entity.
    pub fn extract_access_rights(
        &self,
        entity_hash: AddressableEntityHash,
        named_keys: &NamedKeys,
    ) -> ContextAccessRights {
        let urefs_iter = named_keys
            .keys()
            .filter_map(|key| key.as_uref().copied())
            .chain(iter::once(self.main_purse));
        ContextAccessRights::new(entity_hash, urefs_iter)
    }

    /// Update the byte code hash for a given Entity associated with an Account.
    pub fn update_session_entity(
        self,
        byte_code_hash: ByteCodeHash,
        entry_points: EntryPoints,
    ) -> Self {
        Self {
            package_hash: self.package_hash,
            byte_code_hash,
            entry_points,
            protocol_version: self.protocol_version,
            main_purse: self.main_purse,
            associated_keys: self.associated_keys,
            action_thresholds: self.action_thresholds,
            message_topics: self.message_topics,
            entity_kind: self.entity_kind,
        }
    }
}

impl ToBytes for AddressableEntity {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.package_hash().write_bytes(&mut result)?;
        self.byte_code_hash().write_bytes(&mut result)?;
        self.entry_points().write_bytes(&mut result)?;
        self.protocol_version().write_bytes(&mut result)?;
        self.main_purse().write_bytes(&mut result)?;
        self.associated_keys().write_bytes(&mut result)?;
        self.action_thresholds().write_bytes(&mut result)?;
        self.message_topics().write_bytes(&mut result)?;
        self.entity_kind().write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.entry_points)
            + ToBytes::serialized_length(&self.package_hash)
            + ToBytes::serialized_length(&self.byte_code_hash)
            + ToBytes::serialized_length(&self.protocol_version)
            + ToBytes::serialized_length(&self.main_purse)
            + ToBytes::serialized_length(&self.associated_keys)
            + ToBytes::serialized_length(&self.action_thresholds)
            + ToBytes::serialized_length(&self.message_topics)
            + ToBytes::serialized_length(&self.entity_kind)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.package_hash().write_bytes(writer)?;
        self.byte_code_hash().write_bytes(writer)?;
        self.entry_points().write_bytes(writer)?;
        self.protocol_version().write_bytes(writer)?;
        self.main_purse().write_bytes(writer)?;
        self.associated_keys().write_bytes(writer)?;
        self.action_thresholds().write_bytes(writer)?;
        self.message_topics().write_bytes(writer)?;
        self.entity_kind().write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for AddressableEntity {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (package_hash, bytes) = PackageHash::from_bytes(bytes)?;
        let (byte_code_hash, bytes) = ByteCodeHash::from_bytes(bytes)?;
        let (entry_points, bytes) = EntryPoints::from_bytes(bytes)?;
        let (protocol_version, bytes) = ProtocolVersion::from_bytes(bytes)?;
        let (main_purse, bytes) = URef::from_bytes(bytes)?;
        let (associated_keys, bytes) = AssociatedKeys::from_bytes(bytes)?;
        let (action_thresholds, bytes) = ActionThresholds::from_bytes(bytes)?;
        let (message_topics, bytes) = MessageTopics::from_bytes(bytes)?;
        let (entity_kind, bytes) = EntityKind::from_bytes(bytes)?;
        Ok((
            AddressableEntity {
                package_hash,
                byte_code_hash,
                entry_points,
                protocol_version,
                main_purse,
                associated_keys,
                action_thresholds,
                message_topics,
                entity_kind,
            },
            bytes,
        ))
    }
}

impl Default for AddressableEntity {
    fn default() -> Self {
        AddressableEntity {
            entry_points: EntryPoints::new_with_default_entry_point(),
            byte_code_hash: [0; KEY_HASH_LENGTH].into(),
            package_hash: [0; KEY_HASH_LENGTH].into(),
            protocol_version: ProtocolVersion::V1_0_0,
            main_purse: URef::default(),
            action_thresholds: ActionThresholds::default(),
            associated_keys: AssociatedKeys::default(),
            message_topics: MessageTopics::default(),
            entity_kind: EntityKind::SmartContract,
        }
    }
}

impl From<Contract> for AddressableEntity {
    fn from(value: Contract) -> Self {
        AddressableEntity::new(
            PackageHash::new(value.contract_package_hash().value()),
            ByteCodeHash::new(value.contract_wasm_hash().value()),
            value.entry_points().clone(),
            value.protocol_version(),
            URef::default(),
            AssociatedKeys::default(),
            ActionThresholds::default(),
            MessageTopics::default(),
            EntityKind::SmartContract,
        )
    }
}

impl From<Account> for AddressableEntity {
    fn from(value: Account) -> Self {
        AddressableEntity::new(
            PackageHash::default(),
            ByteCodeHash::new([0u8; 32]),
            EntryPoints::new(),
            ProtocolVersion::default(),
            value.main_purse(),
            value.associated_keys().clone().into(),
            value.action_thresholds().clone().into(),
            MessageTopics::default(),
            EntityKind::Account(value.account_hash()),
        )
    }
}

/// Context of method execution
///
/// Most significant bit represents version i.e.
/// - 0b0 -> 0.x/1.x (session & contracts)
/// - 0b1 -> 2.x and later (introduced installer, utility entry points)
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, FromPrimitive)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EntryPointType {
    /// Runs as session code (caller)
    /// Deprecated, retained to allow read back of legacy stored session.
    Session = 0b00000000,
    /// Runs within called entity's context (called)
    AddressableEntity = 0b00000001,
    /// This entry point is intended to extract a subset of bytecode.
    /// Runs within called entity's context (called)
    Factory = 0b10000000,
}

impl EntryPointType {
    /// Checks if entry point type is introduced before 2.0.
    ///
    /// This method checks if there is a bit pattern for entry point types introduced in 2.0.
    ///
    /// If this bit is missing, that means given entry point type was defined in pre-2.0 world.
    pub fn is_legacy_pattern(&self) -> bool {
        (*self as u8) & 0b10000000 == 0
    }

    /// Get the bit pattern.
    pub fn bits(self) -> u8 {
        self as u8
    }

    /// Returns true if entry point type is invalid for the context.
    pub fn is_invalid_context(&self) -> bool {
        match self {
            EntryPointType::Session => true,
            EntryPointType::AddressableEntity | EntryPointType::Factory => false,
        }
    }
}

impl ToBytes for EntryPointType {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.bits().to_bytes()
    }

    fn serialized_length(&self) -> usize {
        1
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.bits());
        Ok(())
    }
}

impl FromBytes for EntryPointType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, bytes) = u8::from_bytes(bytes)?;
        let entry_point_type =
            EntryPointType::from_u8(value).ok_or(bytesrepr::Error::Formatting)?;
        Ok((entry_point_type, bytes))
    }
}

/// Default name for an entry point.
pub const DEFAULT_ENTRY_POINT_NAME: &str = "call";

/// Name for an installer entry point.
pub const INSTALL_ENTRY_POINT_NAME: &str = "install";

/// Name for an upgrade entry point.
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
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length()
            + self.args.serialized_length()
            + self.ret.serialized_length()
            + self.access.serialized_length()
            + self.entry_point_type.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.name.write_bytes(writer)?;
        self.args.write_bytes(writer)?;
        self.ret.append_bytes(writer)?;
        self.access.write_bytes(writer)?;
        self.entry_point_type.write_bytes(writer)?;
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
    /// Can't be accessed directly but are kept in the derived wasm bytes.
    Template,
}

const ENTRYPOINTACCESS_PUBLIC_TAG: u8 = 1;
const ENTRYPOINTACCESS_GROUPS_TAG: u8 = 2;
const ENTRYPOINTACCESS_ABSTRACT_TAG: u8 = 3;

impl EntryPointAccess {
    /// Constructor for access granted to only listed groups.
    pub fn groups(labels: &[&str]) -> Self {
        let list: Vec<Group> = labels
            .iter()
            .map(|s| Group::new(String::from(*s)))
            .collect();
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
            EntryPointAccess::Template => {
                result.push(ENTRYPOINTACCESS_ABSTRACT_TAG);
            }
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        match self {
            EntryPointAccess::Public => 1,
            EntryPointAccess::Groups(groups) => 1 + groups.serialized_length(),
            EntryPointAccess::Template => 1,
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
            EntryPointAccess::Template => {
                writer.push(ENTRYPOINTACCESS_ABSTRACT_TAG);
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
            ENTRYPOINTACCESS_ABSTRACT_TAG => Ok((EntryPointAccess::Template, bytes)),
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

    #[test]
    fn entity_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let entity_hash = HashAddr::try_from(&bytes[..]).expect("should create contract hash");
        let entity_hash = AddressableEntityHash::new(entity_hash);
        assert_eq!(&bytes, &entity_hash.as_bytes());
    }

    #[test]
    fn entity_hash_from_str() {
        let entity_hash = AddressableEntityHash([3; 32]);
        let encoded = entity_hash.to_formatted_string();
        let decoded = AddressableEntityHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(entity_hash, decoded);

        let invalid_prefix =
            "addressable-entity--0000000000000000000000000000000000000000000000000000000000000000";
        assert!(AddressableEntityHash::from_formatted_str(invalid_prefix).is_err());

        let short_addr =
            "addressable-entity-00000000000000000000000000000000000000000000000000000000000000";
        assert!(AddressableEntityHash::from_formatted_str(short_addr).is_err());

        let long_addr =
            "addressable-entity-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(AddressableEntityHash::from_formatted_str(long_addr).is_err());

        let invalid_hex =
            "addressable-entity-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(AddressableEntityHash::from_formatted_str(invalid_hex).is_err());
    }

    #[test]
    fn named_key_addr_from_str() {
        let named_key_addr = NamedKeyAddr::new_named_key_entry(
            EntityAddr::new_contract_entity_addr([3; 32]),
            [4; 32],
        );
        let encoded = named_key_addr.to_formatted_string();
        let decoded = NamedKeyAddr::from_formatted_str(&encoded).unwrap();
        assert_eq!(named_key_addr, decoded);
    }

    #[test]
    fn formatted_string_roundtrip() {
        let entity_addr = EntityAddr::Account([5; 32]);
        let encoded = entity_addr.to_formatted_string();
        let decoded = EntityAddr::from_formatted_str(&encoded).expect("must get entity addr");
        assert_eq!(decoded, entity_addr);
    }

    #[test]
    fn entity_hash_serde_roundtrip() {
        let entity_hash = AddressableEntityHash([255; 32]);
        let serialized = bincode::serialize(&entity_hash).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(entity_hash, deserialized)
    }

    #[test]
    fn entity_hash_json_roundtrip() {
        let entity_hash = AddressableEntityHash([255; 32]);
        let json_string = serde_json::to_string_pretty(&entity_hash).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(entity_hash, decoded)
    }

    #[test]
    fn serialization_roundtrip() {
        let entity_addr = EntityAddr::new_system_entity_addr([1; 32]);
        bytesrepr::test_serialization_roundtrip(&entity_addr);
        let entity_addr = EntityAddr::new_account_entity_addr([1; 32]);
        bytesrepr::test_serialization_roundtrip(&entity_addr);
        let entity_addr = EntityAddr::new_contract_entity_addr([1; 32]);
        bytesrepr::test_serialization_roundtrip(&entity_addr);
    }

    #[test]
    fn should_extract_access_rights() {
        const MAIN_PURSE: URef = URef::new([2; 32], AccessRights::READ_ADD_WRITE);

        let entity_hash = AddressableEntityHash([255; 32]);
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
            PackageHash::new([254; 32]),
            ByteCodeHash::new([253; 32]),
            EntryPoints::new_with_default_entry_point(),
            ProtocolVersion::V1_0_0,
            MAIN_PURSE,
            associated_keys,
            ActionThresholds::new(Weight::new(1), Weight::new(1), Weight::new(1))
                .expect("should create thresholds"),
            MessageTopics::default(),
            EntityKind::SmartContract,
        );
        let access_rights = contract.extract_access_rights(entity_hash, &named_keys);
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
        #[test]
        fn test_value_contract(contract in gens::addressable_entity_arb()) {
            bytesrepr::test_serialization_roundtrip(&contract);
        }
    }
}
