//! Data types for supporting contract headers feature.
// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

pub mod action_thresholds;
mod action_type;
pub mod associated_keys;
mod entry_points;
mod error;
//mod named_keys;
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

#[cfg(feature = "json-schema")]
use crate::SecretKey;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
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
    entry_points::{
        EntityEntryPoint, EntryPointAccess, EntryPointAddr, EntryPointPayment, EntryPointType,
        EntryPointValue, EntryPoints, Parameter, Parameters, DEFAULT_ENTRY_POINT_NAME,
    },
    error::{FromAccountHashStrError, TryFromIntError, TryFromSliceForAccountHashError},
    weight::{Weight, WEIGHT_SERIALIZED_LENGTH},
};
use crate::{
    account::{
        Account, AccountHash, AddKeyFailure, RemoveKeyFailure, SetThresholdFailure,
        UpdateKeyFailure,
    },
    byte_code::ByteCodeHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex,
    contract_messages::TopicNameHash,
    contracts::{Contract, ContractHash},
    system::SystemEntityType,
    uref::{self, URef},
    AccessRights, ApiError, ByteCodeAddr, CLType, CLTyped, CLValue, CLValueError,
    ContextAccessRights, HashAddr, Key, NamedKeys, PackageAddr, ProtocolVersion, PublicKey, Tagged,
    BLAKE2B_DIGEST_LENGTH, KEY_HASH_LENGTH,
};

/// Maximum number of distinct user groups.
pub const MAX_GROUPS: u8 = 10;
/// Maximum number of URefs which can be assigned across all user groups.
pub const MAX_TOTAL_UREFS: usize = 100;

/// The prefix applied to the hex-encoded `Addressable Entity` to produce a formatted string
/// representation.
pub const ADDRESSABLE_ENTITY_STRING_PREFIX: &str = "addressable-entity-";
/// The prefix applied to the hex-encoded `Entity` to produce a formatted string
/// representation.
pub const ENTITY_PREFIX: &str = "entity-";
/// The prefix applied to the hex-encoded `Account` to produce a formatted string
/// representation.
pub const ACCOUNT_ENTITY_PREFIX: &str = "account-";
/// The prefix applied to the hex-encoded `Smart contract` to produce a formatted string
/// representation.
pub const CONTRACT_ENTITY_PREFIX: &str = "contract-";
/// The prefix applied to the hex-encoded `System entity account or contract` to produce a formatted
///  string representation.
pub const SYSTEM_ENTITY_PREFIX: &str = "system-";
/// The prefix applied to the hex-encoded `Named Key` to produce a formatted string
/// representation.
pub const NAMED_KEY_PREFIX: &str = "named-key-";
pub const PACKAGE_ENTITY_PREFIX: &str = "package-";

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

    /// Get the entity addr for this entity hash from the corresponding entity.
    pub fn entity_addr(&self, entity: AddressableEntity) -> EntityAddr {
        entity.entity_addr(*self)
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

    /// Hexadecimal representation of the hash.
    pub fn to_hex_string(&self) -> String {
        base16::encode_lower(&self.0)
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
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for AddressableEntityHash {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
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

/// Tag for the variants of [`EntityKind`].
#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[repr(u8)]
pub enum EntityKindTag {
    /// `EntityKind::System` variant.
    System = 0,
    /// `EntityKind::Account` variant.
    Account = 1,
    /// `EntityKind::SmartContract` variant.
    SmartContract = 2,
    Package = 3,
}

impl TryFrom<u8> for EntityKindTag {
    type Error = bytesrepr::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(EntityKindTag::System),
            1 => Ok(EntityKindTag::Account),
            2 => Ok(EntityKindTag::SmartContract),
            3 => Ok(EntityKindTag::Package),
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
            EntityKindTag::Package => {
                write!(f, "package")
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

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Runtime used to execute a Transaction.")
)]
//Default is needed only in testing to meet EnumIter needs
#[cfg_attr(any(feature = "testing", test), derive(Default))]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum ContractRuntimeTag {
    #[cfg_attr(any(feature = "testing", test), default)]
    VmCasperV1,
    VmCasperV2,
}

#[cfg(any(feature = "testing", test))]
impl Distribution<ContractRuntimeTag> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ContractRuntimeTag {
        match rng.gen_range(0..=1) {
            0 => ContractRuntimeTag::VmCasperV1,
            1 => ContractRuntimeTag::VmCasperV2,
            _ => unreachable!(),
        }
    }
}

impl ToBytes for ContractRuntimeTag {
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

impl FromBytes for ContractRuntimeTag {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        if tag == ContractRuntimeTag::VmCasperV1 as u8 {
            Ok((ContractRuntimeTag::VmCasperV1, remainder))
        } else if tag == ContractRuntimeTag::VmCasperV2 as u8 {
            Ok((ContractRuntimeTag::VmCasperV2, remainder))
        } else {
            Err(bytesrepr::Error::Formatting)
        }
    }
}

impl Display for ContractRuntimeTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractRuntimeTag::VmCasperV1 => write!(f, "vm-casper-v1"),
            ContractRuntimeTag::VmCasperV2 => write!(f, "vm-casper-v2"),
        }
    }
}
impl ContractRuntimeTag {
    /// Returns the tag of the [`ContractRuntimeTag`].
    pub fn tag(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
/// The type of Package.
pub enum EntityKind {
    /// Package associated with a native contract implementation.
    System(SystemEntityType),
    /// Package associated with an Account hash.
    Account(AccountHash),
    /// Packages associated with Wasm stored on chain.
    SmartContract(ContractRuntimeTag),
    Package(PackageAddr),
}

impl EntityKind {
    /// Returns the Account hash associated with a Package based on the package kind.
    pub fn maybe_account_hash(&self) -> Option<AccountHash> {
        match self {
            Self::Account(account_hash) => Some(*account_hash),
            _ => None,
        }
    }

    /// Returns the associated key set based on the Account hash set in the package kind.
    pub fn associated_keys(&self) -> AssociatedKeys {
        match self {
            Self::Account(account_hash) => AssociatedKeys::new(*account_hash, Weight::new(1)),
            _ => AssociatedKeys::default(),
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
            EntityKind::SmartContract(_) => EntityKindTag::SmartContract,
            EntityKind::Package(_) => EntityKindTag::Package,
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
                EntityKind::SmartContract(transaction_runtime) => {
                    transaction_runtime.serialized_length()
                }
                EntityKind::System(system_entity_type) => system_entity_type.serialized_length(),
                EntityKind::Account(account_hash) => account_hash.serialized_length(),
                EntityKind::Package(package_hash) => package_hash.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        match self {
            EntityKind::SmartContract(transaction_runtime) => {
                transaction_runtime.write_bytes(writer)
            }
            EntityKind::System(system_entity_type) => system_entity_type.write_bytes(writer),
            EntityKind::Account(account_hash) => account_hash.write_bytes(writer),
            EntityKind::Package(package_hash) => package_hash.write_bytes(writer),
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
            EntityKindTag::SmartContract => {
                let (transaction_runtime, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((EntityKind::SmartContract(transaction_runtime), remainder))
            }
            EntityKindTag::Package => {
                let (package_hash, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((EntityKind::Package(package_hash), remainder))
            }
        }
    }
}

impl Display for EntityKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EntityKind::System(system_entity) => {
                write!(f, "system-entity-kind({})", system_entity)
            }
            EntityKind::Account(account_hash) => {
                write!(f, "account-entity-kind({})", account_hash)
            }
            EntityKind::SmartContract(transaction_runtime) => {
                write!(f, "smart-contract-entity-kind({})", transaction_runtime)
            }
            EntityKind::Package(package_hash) => {
                write!(f, "package-entity-kind({})", package_hash)
            }
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<EntityKind> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EntityKind {
        match rng.gen_range(0..=3) {
            0 => EntityKind::System(rng.gen()),
            1 => EntityKind::Account(rng.gen()),
            2 => EntityKind::SmartContract(rng.gen()),
            3 => EntityKind::Package(PackageAddr::default()),
            _ => unreachable!(),
        }
    }
}

/// The address for an AddressableEntity which contains the 32 bytes and tagging information.
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema), schemars(untagged))]
pub enum EntityAddr {
    /// The address for a system entity contract.
    // DO NOT USE FOR SYSTEM ACCOUNT, ONLY SYSTEM CONTRACTS
    System(#[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))] HashAddr),
    /// The address of an entity that corresponds to an Account.
    Account(#[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))] HashAddr),
    /// The address of an entity that corresponds to a Userland smart contract.
    SmartContract(#[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))] HashAddr),
    /// The address of an entity that corresponds to a Userland smart contract package.
    Package(#[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))] HashAddr),
}

impl EntityAddr {
    /// The length in bytes of an `EntityAddr`.
    pub const LENGTH: usize = U8_SERIALIZED_LENGTH + KEY_HASH_LENGTH;

    /// Constructs a new `EntityAddr` for a system entity.
    pub const fn new_system(hash_addr: HashAddr) -> Self {
        Self::System(hash_addr)
    }

    /// Constructs a new `EntityAddr` for an Account entity.
    pub const fn new_account(hash_addr: HashAddr) -> Self {
        Self::Account(hash_addr)
    }

    /// Constructs a new `EntityAddr` for a smart contract.
    pub const fn new_smart_contract(hash_addr: HashAddr) -> Self {
        Self::SmartContract(hash_addr)
    }

    pub const fn new_package(hash_addr: HashAddr) -> Self {
        Self::Package(hash_addr)
    }

    /// Constructs a new `EntityAddr` based on the supplied kind.
    pub fn new_of_kind(entity_kind: EntityKind, hash_addr: HashAddr) -> Self {
        match entity_kind {
            EntityKind::System(_) => Self::new_system(hash_addr),
            EntityKind::Account(_) => Self::new_account(hash_addr),
            EntityKind::SmartContract(_) => Self::new_smart_contract(hash_addr),
            EntityKind::Package(_) => Self::new_package(hash_addr),
        }
    }

    /// Returns the tag of the [`EntityAddr`].
    pub fn tag(&self) -> EntityKindTag {
        match self {
            EntityAddr::System(_) => EntityKindTag::System,
            EntityAddr::Account(_) => EntityKindTag::Account,
            EntityAddr::SmartContract(_) => EntityKindTag::SmartContract,
            EntityAddr::Package(_) => EntityKindTag::Package,
        }
    }

    /// Is this a system entity address?
    pub fn is_system(&self) -> bool {
        self.tag() == EntityKindTag::System
            || self.value() == PublicKey::System.to_account_hash().value()
    }

    /// Is this a contract entity address?
    pub fn is_contract(&self) -> bool {
        self.tag() == EntityKindTag::SmartContract
    }

    /// Is this an account entity address?
    pub fn is_account(&self) -> bool {
        self.tag() == EntityKindTag::Account
    }

    /// Returns the 32 bytes of the [`EntityAddr`].
    pub fn value(&self) -> HashAddr {
        match self {
            EntityAddr::System(hash_addr)
            | EntityAddr::Account(hash_addr)
            | EntityAddr::SmartContract(hash_addr)
            | EntityAddr::Package(hash_addr) => *hash_addr,
        }
    }

    /// Returns the formatted String representation of the [`EntityAddr`].
    pub fn to_formatted_string(&self) -> String {
        match self {
            EntityAddr::System(addr) => {
                format!(
                    "{}{}{}",
                    ENTITY_PREFIX,
                    SYSTEM_ENTITY_PREFIX,
                    base16::encode_lower(addr)
                )
            }
            EntityAddr::Account(addr) => {
                format!(
                    "{}{}{}",
                    ENTITY_PREFIX,
                    ACCOUNT_ENTITY_PREFIX,
                    base16::encode_lower(addr)
                )
            }
            EntityAddr::SmartContract(addr) => {
                format!(
                    "{}{}{}",
                    ENTITY_PREFIX,
                    CONTRACT_ENTITY_PREFIX,
                    base16::encode_lower(addr)
                )
            }
            EntityAddr::Package(addr) => {
                format!(
                    "{}{}{}",
                    ENTITY_PREFIX,
                    PACKAGE_ENTITY_PREFIX,
                    base16::encode_lower(addr)
                )
            }
        }
    }

    /// Constructs an [`EntityAddr`] from a formatted String.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        if let Some(entity) = input.strip_prefix(ENTITY_PREFIX) {
            let (addr_str, tag) = if let Some(str) = entity.strip_prefix(SYSTEM_ENTITY_PREFIX) {
                (str, EntityKindTag::System)
            } else if let Some(str) = entity.strip_prefix(ACCOUNT_ENTITY_PREFIX) {
                (str, EntityKindTag::Account)
            } else if let Some(str) = entity.strip_prefix(CONTRACT_ENTITY_PREFIX) {
                (str, EntityKindTag::SmartContract)
            } else {
                return Err(FromStrError::InvalidPrefix);
            };
            let addr = checksummed_hex::decode(addr_str).map_err(FromStrError::Hex)?;
            let hash_addr = HashAddr::try_from(addr.as_ref()).map_err(FromStrError::Hash)?;
            let entity_addr = match tag {
                EntityKindTag::System => EntityAddr::new_system(hash_addr),
                EntityKindTag::Account => EntityAddr::new_account(hash_addr),
                EntityKindTag::SmartContract => EntityAddr::new_smart_contract(hash_addr),
                EntityKindTag::Package => EntityAddr::new_package(hash_addr),
            };

            return Ok(entity_addr);
        }

        Err(FromStrError::InvalidPrefix)
    }

    pub fn into_smart_contract(&self) -> Option<[u8; 32]> {
        match self {
            EntityAddr::SmartContract(addr) => Some(*addr),
            _ => None,
        }
    }
}

impl ToBytes for EntityAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        EntityAddr::LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            EntityAddr::System(addr) => {
                EntityKindTag::System.write_bytes(writer)?;
                addr.write_bytes(writer)
            }
            EntityAddr::Account(addr) => {
                EntityKindTag::Account.write_bytes(writer)?;
                addr.write_bytes(writer)
            }
            EntityAddr::SmartContract(addr) => {
                EntityKindTag::SmartContract.write_bytes(writer)?;
                addr.write_bytes(writer)
            }
            EntityAddr::Package(addr) => {
                EntityKindTag::Package.write_bytes(writer)?;
                addr.write_bytes(writer)
            }
        }
    }
}

impl FromBytes for EntityAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = EntityKindTag::from_bytes(bytes)?;
        let (addr, remainder) = HashAddr::from_bytes(remainder)?;
        let entity_addr = match tag {
            EntityKindTag::System => EntityAddr::System(addr),
            EntityKindTag::Account => EntityAddr::Account(addr),
            EntityKindTag::SmartContract => EntityAddr::SmartContract(addr),
            EntityKindTag::Package => EntityAddr::Package(addr),
        };
        Ok((entity_addr, remainder))
    }
}

impl CLTyped for EntityAddr {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl From<EntityAddr> for AddressableEntityHash {
    fn from(entity_addr: EntityAddr) -> Self {
        AddressableEntityHash::new(entity_addr.value())
    }
}

impl Display for EntityAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_formatted_string())
    }
}

impl Debug for EntityAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EntityAddr::System(hash_addr) => {
                write!(f, "EntityAddr::System({})", base16::encode_lower(hash_addr))
            }
            EntityAddr::Account(hash_addr) => {
                write!(
                    f,
                    "EntityAddr::Account({})",
                    base16::encode_lower(hash_addr)
                )
            }
            EntityAddr::SmartContract(hash_addr) => {
                write!(
                    f,
                    "EntityAddr::SmartContract({})",
                    base16::encode_lower(hash_addr)
                )
            }
            EntityAddr::Package(hash_addr) => {
                write!(
                    f,
                    "EntityAddr::SmartContract({})",
                    base16::encode_lower(hash_addr)
                )
            }
        }
    }
}

impl Serialize for EntityAddr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            let (tag, value): (EntityKindTag, HashAddr) = (self.tag(), self.value());
            (tag, value).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for EntityAddr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            Self::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let (tag, addr) = <(EntityKindTag, HashAddr)>::deserialize(deserializer)?;
            match tag {
                EntityKindTag::System => Ok(EntityAddr::new_system(addr)),
                EntityKindTag::Account => Ok(EntityAddr::new_account(addr)),
                EntityKindTag::SmartContract => Ok(EntityAddr::new_smart_contract(addr)),
                EntityKindTag::Package => Ok(EntityAddr::new_package(addr)),
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
    pub const NAMED_KEY_ADDR_BASE_LENGTH: usize = 1 + EntityAddr::LENGTH;

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
        let mut hasher = {
            match VarBlake2b::new(BLAKE2B_DIGEST_LENGTH) {
                Ok(hasher) => hasher,
                Err(_) => return Err(bytesrepr::Error::Formatting),
            }
        };
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

impl Default for NamedKeyAddr {
    fn default() -> Self {
        NamedKeyAddr {
            base_addr: EntityAddr::System(HashAddr::default()),
            string_bytes: Default::default(),
        }
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

#[cfg(any(feature = "testing", test))]
impl Distribution<NamedKeyAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> NamedKeyAddr {
        NamedKeyAddr {
            base_addr: rng.gen(),
            string_bytes: rng.gen(),
        }
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

#[cfg(feature = "json-schema")]
static ADDRESSABLE_ENTITY: Lazy<AddressableEntity> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([0; 32]).unwrap();
    let account_hash = PublicKey::from(&secret_key).to_account_hash();
    let package = PackageAddr::new([0; 32]);
    let byte_code = ByteCodeHash::new([0; 32]);
    let main_purse = URef::from_formatted_str(
        "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007",
    )
    .unwrap();
    let weight = Weight::new(1);
    let associated_keys = AssociatedKeys::new(account_hash, weight);
    let action_thresholds = ActionThresholds::new(weight, weight, weight).unwrap();
    let protocol_version = ProtocolVersion::from_parts(2, 0, 0);
    AddressableEntity {
        protocol_version,
        entity_kind: EntityKind::Account(account_hash),
        package,
        byte_code,
        main_purse,
        associated_keys,
        action_thresholds,
    }
});

/// The address for an AddressableEntity which contains the 32 bytes and tagging information.
pub type ContractAddress = PackageAddr;

/// Methods and type signatures supported by a contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct AddressableEntity {
    protocol_version: ProtocolVersion,
    entity_kind: EntityKind,
    package: PackageAddr,
    byte_code: ByteCodeHash,
    main_purse: URef,

    associated_keys: AssociatedKeys,
    action_thresholds: ActionThresholds,
}

impl AddressableEntity {
    /// `AddressableEntity` constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        package: PackageAddr,
        byte_code: ByteCodeHash,
        protocol_version: ProtocolVersion,
        main_purse: URef,
        associated_keys: AssociatedKeys,
        action_thresholds: ActionThresholds,
        entity_kind: EntityKind,
    ) -> Self {
        AddressableEntity {
            package,
            byte_code,
            protocol_version,
            main_purse,
            action_thresholds,
            associated_keys,
            entity_kind,
        }
    }

    /// Get the entity addr for this entity from the corresponding hash.
    pub fn entity_addr(&self, entity_hash: AddressableEntityHash) -> EntityAddr {
        let hash_addr = entity_hash.value();
        match self.entity_kind {
            EntityKind::System(_) => EntityAddr::new_system(hash_addr),
            EntityKind::Account(_) => EntityAddr::new_account(hash_addr),
            EntityKind::SmartContract(_) => EntityAddr::new_smart_contract(hash_addr),
            EntityKind::Package(_) => EntityAddr::new_package(hash_addr),
        }
    }

    pub fn entity_kind(&self) -> EntityKind {
        self.entity_kind
    }

    /// Hash for accessing contract package
    pub fn package(&self) -> PackageAddr {
        self.package
    }

    /// Hash for accessing contract WASM
    pub fn byte_code(&self) -> ByteCodeHash {
        self.byte_code
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

    /// Set protocol_version.
    pub fn set_protocol_version(&mut self, protocol_version: ProtocolVersion) {
        self.protocol_version = protocol_version;
    }

    /// Determines if `AddressableEntity` is compatible with a given `ProtocolVersion`.
    pub fn is_compatible_protocol_version(&self, protocol_version: ProtocolVersion) -> bool {
        let entity_protocol_version = self.protocol_version.value();
        let context_protocol_version = protocol_version.value();
        if entity_protocol_version.major == context_protocol_version.major {
            return true;
        }
        if entity_protocol_version.major == 1 && context_protocol_version.major == 2 {
            // the 1.x model has been deprecated but is still supported until 3.0.0
            return true;
        }
        false
    }

    /// Returns the kind of `AddressableEntity`.
    pub fn kind(&self) -> EntityKind {
        self.entity_kind
    }

    /// Is this an account?
    pub fn is_account_kind(&self) -> bool {
        matches!(self.entity_kind, EntityKind::Account(_))
    }

    /// Is this a contract?
    pub fn is_smart_contract_kind(&self) -> bool {
        matches!(self.entity_kind, EntityKind::SmartContract(_))
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
            EntityKind::SmartContract(_) => {
                Key::addressable_entity_key(EntityKindTag::SmartContract, entity_hash)
            }
            EntityKind::Package(_) => Key::Package(entity_hash.value().into()),
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
        ContextAccessRights::new(entity_hash.value(), urefs_iter)
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &ADDRESSABLE_ENTITY
    }

    /// Addr for accessing wasm bytes
    pub fn byte_code_addr(&self) -> Option<ByteCodeAddr> {
        match self.entity_kind {
            EntityKind::System(_) => Some(ByteCodeAddr::Empty),
            EntityKind::Account(_) => None,
            EntityKind::SmartContract(contract_runtime_tag) => match contract_runtime_tag {
                ContractRuntimeTag::VmCasperV1 => {
                    Some(ByteCodeAddr::V1CasperWasm(self.byte_code.value()))
                }
                ContractRuntimeTag::VmCasperV2 => {
                    Some(ByteCodeAddr::V2CasperWasm(self.byte_code.value()))
                }
            },
            EntityKind::Package(_) => None,
        }
    }
}

impl From<AddressableEntity>
    for (
        PackageAddr,
        ByteCodeHash,
        ProtocolVersion,
        URef,
        AssociatedKeys,
        ActionThresholds,
    )
{
    fn from(entity: AddressableEntity) -> Self {
        (
            entity.package,
            entity.byte_code,
            entity.protocol_version,
            entity.main_purse,
            entity.associated_keys,
            entity.action_thresholds,
        )
    }
}

impl ToBytes for AddressableEntity {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.package().write_bytes(&mut result)?;
        self.byte_code().write_bytes(&mut result)?;
        self.protocol_version().write_bytes(&mut result)?;
        self.main_purse().write_bytes(&mut result)?;
        self.associated_keys().write_bytes(&mut result)?;
        self.action_thresholds().write_bytes(&mut result)?;
        self.kind().write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.package)
            + ToBytes::serialized_length(&self.byte_code)
            + ToBytes::serialized_length(&self.protocol_version)
            + ToBytes::serialized_length(&self.main_purse)
            + ToBytes::serialized_length(&self.associated_keys)
            + ToBytes::serialized_length(&self.action_thresholds)
            + ToBytes::serialized_length(&self.entity_kind)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.package().write_bytes(writer)?;
        self.byte_code().write_bytes(writer)?;
        self.protocol_version().write_bytes(writer)?;
        self.main_purse().write_bytes(writer)?;
        self.associated_keys().write_bytes(writer)?;
        self.action_thresholds().write_bytes(writer)?;
        self.kind().write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for AddressableEntity {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (package, bytes) = PackageAddr::from_bytes(bytes)?;
        let (byte_code, bytes) = ByteCodeHash::from_bytes(bytes)?;
        let (protocol_version, bytes) = ProtocolVersion::from_bytes(bytes)?;
        let (main_purse, bytes) = URef::from_bytes(bytes)?;
        let (associated_keys, bytes) = AssociatedKeys::from_bytes(bytes)?;
        let (action_thresholds, bytes) = ActionThresholds::from_bytes(bytes)?;
        let (entity_kind, bytes) = EntityKind::from_bytes(bytes)?;
        Ok((
            AddressableEntity {
                package,
                byte_code,
                protocol_version,
                main_purse,
                associated_keys,
                action_thresholds,
                entity_kind,
            },
            bytes,
        ))
    }
}

impl Default for AddressableEntity {
    fn default() -> Self {
        AddressableEntity {
            byte_code: [0; KEY_HASH_LENGTH].into(),
            package: [0; KEY_HASH_LENGTH].into(),
            protocol_version: ProtocolVersion::V1_0_0,
            main_purse: URef::default(),
            action_thresholds: ActionThresholds::default(),
            associated_keys: AssociatedKeys::default(),
            entity_kind: EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1),
        }
    }
}

impl From<Contract> for AddressableEntity {
    fn from(value: Contract) -> Self {
        AddressableEntity::new(
            PackageAddr::new(value.contract_package_hash().value()),
            ByteCodeHash::new(value.contract_wasm_hash().value()),
            value.protocol_version(),
            URef::default(),
            AssociatedKeys::default(),
            ActionThresholds::default(),
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1),
        )
    }
}

impl From<Account> for AddressableEntity {
    fn from(value: Account) -> Self {
        AddressableEntity::new(
            PackageAddr::default(),
            ByteCodeHash::new([0u8; 32]),
            ProtocolVersion::default(),
            value.main_purse(),
            value.associated_keys().clone().into(),
            value.action_thresholds().clone().into(),
            EntityKind::Account(value.account_hash()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessRights, URef, UREF_ADDR_LENGTH};

    #[cfg(feature = "json-schema")]
    use schemars::{gen::SchemaGenerator, schema::InstanceType};

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
        let named_key_addr =
            NamedKeyAddr::new_named_key_entry(EntityAddr::new_smart_contract([3; 32]), [4; 32]);
        let encoded = named_key_addr.to_formatted_string();
        let decoded = NamedKeyAddr::from_formatted_str(&encoded).unwrap();
        assert_eq!(named_key_addr, decoded);
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
    fn entity_addr_formatted_string_roundtrip() {
        let entity_addr = EntityAddr::Account([5; 32]);
        let encoded = entity_addr.to_formatted_string();
        let decoded = EntityAddr::from_formatted_str(&encoded).expect("must get entity addr");
        assert_eq!(decoded, entity_addr);

        let entity_addr = EntityAddr::SmartContract([5; 32]);
        let encoded = entity_addr.to_formatted_string();
        let decoded = EntityAddr::from_formatted_str(&encoded).expect("must get entity addr");
        assert_eq!(decoded, entity_addr);

        let entity_addr = EntityAddr::System([5; 32]);
        let encoded = entity_addr.to_formatted_string();
        let decoded = EntityAddr::from_formatted_str(&encoded).expect("must get entity addr");
        assert_eq!(decoded, entity_addr);
    }

    #[test]
    fn entity_addr_serialization_roundtrip() {
        for addr in [
            EntityAddr::new_system([1; 32]),
            EntityAddr::new_account([1; 32]),
            EntityAddr::new_smart_contract([1; 32]),
        ] {
            bytesrepr::test_serialization_roundtrip(&addr);
        }
    }

    #[test]
    fn entity_addr_serde_roundtrip() {
        for addr in [
            EntityAddr::new_system([1; 32]),
            EntityAddr::new_account([1; 32]),
            EntityAddr::new_smart_contract([1; 32]),
        ] {
            let serialized = bincode::serialize(&addr).unwrap();
            let deserialized = bincode::deserialize(&serialized).unwrap();
            assert_eq!(addr, deserialized)
        }
    }

    #[test]
    fn entity_addr_json_roundtrip() {
        for addr in [
            EntityAddr::new_system([1; 32]),
            EntityAddr::new_account([1; 32]),
            EntityAddr::new_smart_contract([1; 32]),
        ] {
            let json_string = serde_json::to_string_pretty(&addr).unwrap();
            let decoded = serde_json::from_str(&json_string).unwrap();
            assert_eq!(addr, decoded)
        }
    }

    #[cfg(feature = "json-schema")]
    #[test]
    fn entity_addr_schema() {
        let mut gen = SchemaGenerator::default();
        let any_of = EntityAddr::json_schema(&mut gen)
            .into_object()
            .subschemas
            .expect("should have subschemas")
            .any_of
            .expect("should have any_of");
        for elem in any_of {
            let schema = elem
                .into_object()
                .instance_type
                .expect("should have instance type");
            assert!(schema.contains(&InstanceType::String), "{:?}", schema);
        }
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
            PackageAddr::new([254; 32]),
            ByteCodeHash::new([253; 32]),
            ProtocolVersion::V1_0_0,
            MAIN_PURSE,
            associated_keys,
            ActionThresholds::new(Weight::new(1), Weight::new(1), Weight::new(1))
                .expect("should create thresholds"),
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1),
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
