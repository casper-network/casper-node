//! Module containing the Package and associated types for addressable entities.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    vec::Vec,
};
use core::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "json-schema")]
use serde_map_to_array::KeyValueJsonSchema;
use serde_map_to_array::{BTreeMapToArray, KeyValueLabels};

use crate::{
    account::AccountHash,
    addressable_entity::{AssociatedKeys, Error, FromStrError, Weight},
    bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH},
    checksummed_hex,
    crypto::{self, PublicKey},
    system::SystemContractType,
    uref::URef,
    AddressableEntityHash, CLType, CLTyped, HashAddr, BLAKE2B_DIGEST_LENGTH, KEY_HASH_LENGTH,
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

const PACKAGE_STRING_PREFIX: &str = "contract-package-";
// We need to support the legacy prefix of "contract-package-wasm".
const PACKAGE_STRING_LEGACY_EXTRA_PREFIX: &str = "wasm";

/// Associated error type of `TryFrom<&[u8]>` for `ContractHash`.
#[derive(Debug)]
pub struct TryFromSliceForPackageHashError(());

impl Display for TryFromSliceForPackageHashError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "failed to retrieve from slice")
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct ContractVersionKey {
    /// Major element of `ProtocolVersion` a `ContractVersion` is compatible with.
    protocol_version_major: ProtocolVersionMajor,
    /// Automatically incremented value for a contract version within a major `ProtocolVersion`.
    contract_version: ContractVersion,
}

impl ContractVersionKey {
    /// Returns a new instance of ContractVersionKey with provided values.
    pub fn new(
        protocol_version_major: ProtocolVersionMajor,
        contract_version: ContractVersion,
    ) -> Self {
        Self {
            protocol_version_major,
            contract_version,
        }
    }

    /// Returns the major element of the protocol version this contract is compatible with.
    pub fn protocol_version_major(self) -> ProtocolVersionMajor {
        self.protocol_version_major
    }

    /// Returns the contract version within the protocol major version.
    pub fn contract_version(self) -> ContractVersion {
        self.contract_version
    }
}

impl From<ContractVersionKey> for (ProtocolVersionMajor, ContractVersion) {
    fn from(contract_version_key: ContractVersionKey) -> Self {
        (
            contract_version_key.protocol_version_major,
            contract_version_key.contract_version,
        )
    }
}

impl ToBytes for ContractVersionKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        CONTRACT_VERSION_KEY_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.protocol_version_major.write_bytes(writer)?;
        self.contract_version.write_bytes(writer)
    }
}

impl FromBytes for ContractVersionKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version_major, remainder) = ProtocolVersionMajor::from_bytes(bytes)?;
        let (contract_version, remainder) = ContractVersion::from_bytes(remainder)?;
        Ok((
            ContractVersionKey {
                protocol_version_major,
                contract_version,
            },
            remainder,
        ))
    }
}

impl Display for ContractVersionKey {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{}.{}",
            self.protocol_version_major, self.contract_version
        )
    }
}

/// Serialized length of `ContractVersionKey`.
pub const CONTRACT_VERSION_KEY_SERIALIZED_LENGTH: usize =
    U32_SERIALIZED_LENGTH + U32_SERIALIZED_LENGTH;

/// Collection of contract versions.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(transparent, deny_unknown_fields)]
pub struct ContractVersions(
    #[serde(
        with = "BTreeMapToArray::<ContractVersionKey, AddressableEntityHash, ContractVersionLabels>"
    )]
    BTreeMap<ContractVersionKey, AddressableEntityHash>,
);

impl ContractVersions {
    /// Constructs a new, empty `ContractVersions`.
    pub const fn new() -> Self {
        ContractVersions(BTreeMap::new())
    }

    /// Returns an iterator over the `ContractHash`s (i.e. the map's values).
    pub fn contract_hashes(&self) -> impl Iterator<Item = &AddressableEntityHash> {
        self.0.values()
    }

    /// Returns the `ContractHash` under the key
    pub fn get(&self, key: &ContractVersionKey) -> Option<&AddressableEntityHash> {
        self.0.get(key)
    }
}

impl ToBytes for ContractVersions {
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

impl FromBytes for ContractVersions {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (versions, remainder) =
            BTreeMap::<ContractVersionKey, AddressableEntityHash>::from_bytes(bytes)?;
        Ok((ContractVersions(versions), remainder))
    }
}

#[cfg(any(feature = "testing", feature = "gens", test))]
impl From<BTreeMap<ContractVersionKey, AddressableEntityHash>> for ContractVersions {
    fn from(value: BTreeMap<ContractVersionKey, AddressableEntityHash>) -> Self {
        ContractVersions(value)
    }
}

struct ContractVersionLabels;

impl KeyValueLabels for ContractVersionLabels {
    const KEY: &'static str = "contract_version_key";
    const VALUE: &'static str = "contract_hash";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for ContractVersionLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("ContractVersionAndHash");
}

/// Collection of named groups.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(transparent, deny_unknown_fields)]
pub struct Groups(
    #[serde(with = "BTreeMapToArray::<Group, BTreeSet::<URef>, GroupLabels>")]
    BTreeMap<Group, BTreeSet<URef>>,
);

impl Groups {
    /// Constructs a new, empty `Groups`.
    pub const fn new() -> Self {
        Groups(BTreeMap::new())
    }

    /// Inserts a named group.
    ///
    /// If the map did not have this name present, `None` is returned.  If the map did have this
    /// name present, its collection of `URef`s is overwritten, and the collection is returned.
    pub fn insert(&mut self, name: Group, urefs: BTreeSet<URef>) -> Option<BTreeSet<URef>> {
        self.0.insert(name, urefs)
    }

    /// Returns `true` if the named group exists in the collection.
    pub fn contains(&self, name: &Group) -> bool {
        self.0.contains_key(name)
    }

    /// Returns a reference to the collection of `URef`s under the given `name` if any.
    pub fn get(&self, name: &Group) -> Option<&BTreeSet<URef>> {
        self.0.get(name)
    }

    /// Returns a mutable reference to the collection of `URef`s under the given `name` if any.
    pub fn get_mut(&mut self, name: &Group) -> Option<&mut BTreeSet<URef>> {
        self.0.get_mut(name)
    }

    /// Returns the number of named groups.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if there are no named groups.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an iterator over the `Key`s (i.e. the map's values).
    pub fn keys(&self) -> impl Iterator<Item = &BTreeSet<URef>> {
        self.0.values()
    }

    /// Returns the total number of `URef`s contained in all the groups.
    pub fn total_urefs(&self) -> usize {
        self.0.values().map(|urefs| urefs.len()).sum()
    }
}

impl ToBytes for Groups {
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

impl FromBytes for Groups {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (groups, remainder) = BTreeMap::<Group, BTreeSet<URef>>::from_bytes(bytes)?;
        Ok((Groups(groups), remainder))
    }
}

struct GroupLabels;

impl KeyValueLabels for GroupLabels {
    const KEY: &'static str = "group_name";
    const VALUE: &'static str = "group_users";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for GroupLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("NamedUserGroup");
}

#[cfg(any(feature = "testing", feature = "gens", test))]
impl From<BTreeMap<Group, BTreeSet<URef>>> for Groups {
    fn from(value: BTreeMap<Group, BTreeSet<URef>>) -> Self {
        Groups(value)
    }
}

/// A newtype wrapping a `HashAddr` which references a [`Package`] in the global state.
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The hex-encoded address of the contract package.")
)]
pub struct ContractPackageHash(
    #[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))] HashAddr,
);

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
    type Error = TryFromSliceForPackageHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForPackageHashError> {
        HashAddr::try_from(bytes)
            .map(ContractPackageHash::new)
            .map_err(|_| TryFromSliceForPackageHashError(()))
    }
}

impl TryFrom<&Vec<u8>> for ContractPackageHash {
    type Error = TryFromSliceForPackageHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        HashAddr::try_from(bytes as &[u8])
            .map(ContractPackageHash::new)
            .map_err(|_| TryFromSliceForPackageHashError(()))
    }
}

impl From<&PublicKey> for ContractPackageHash {
    fn from(public_key: &PublicKey) -> Self {
        ContractPackageHash::from_public_key(public_key, crypto::blake2b)
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
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

    /// Returns if the current package is either a system contract or the system entity.
    pub fn is_system(&self) -> bool {
        matches!(self, Self::System(_))
    }

    /// Returns if the current package is the system mint.
    pub fn is_system_mint(&self) -> bool {
        matches!(self, Self::System(SystemContractType::Mint))
    }

    /// Returns if the current package is the system auction.
    pub fn is_system_auction(&self) -> bool {
        matches!(self, Self::System(SystemContractType::Auction))
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

impl ToBytes for ContractPackageKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
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

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ContractPackageKind::Wasm => PACKAGE_KIND_WASM_TAG.write_bytes(writer),
            ContractPackageKind::System(system_contract_type) => {
                PACKAGE_KIND_SYSTEM_CONTRACT_TAG.write_bytes(writer)?;
                system_contract_type.write_bytes(writer)
            }
            ContractPackageKind::Account(account_hash) => {
                PACKAGE_KIND_ACCOUNT_TAG.write_bytes(writer)?;
                account_hash.write_bytes(writer)
            }
            ContractPackageKind::Legacy => PACKAGE_KIND_LEGACY_TAG.write_bytes(writer),
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

impl Display for ContractPackageKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractPackageKind::Wasm => {
                write!(f, "ContractPackageKind:Wasm")
            }
            ContractPackageKind::System(system_contract) => {
                write!(f, "ContractPackageKind:System({})", system_contract)
            }
            ContractPackageKind::Account(account_hash) => {
                write!(f, "ContractPackageKind:Account({})", account_hash)
            }
            ContractPackageKind::Legacy => {
                write!(f, "ContractPackageKind:Legacy")
            }
        }
    }
}

/// Contract definition, metadata, and security container.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Package {
    /// Key used to add or disable versions.
    access_key: URef,
    /// All versions (enabled & disabled).
    versions: ContractVersions,
    /// Collection of disabled contract versions. The runtime will not permit disabled contract
    /// versions to be executed.
    disabled_versions: BTreeSet<ContractVersionKey>,
    /// Mapping maintaining the set of URefs associated with each "user group". This can be used to
    /// control access to methods in a particular version of the contract. A method is callable by
    /// any context which "knows" any of the URefs associated with the method's user group.
    groups: Groups,
    /// A flag that determines whether a contract is locked
    lock_status: ContractPackageStatus,
    /// The kind of package.
    contract_package_kind: ContractPackageKind,
}

impl CLTyped for Package {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl Package {
    /// Create new `Package` (with no versions) from given access key.
    pub fn new(
        access_key: URef,
        versions: ContractVersions,
        disabled_versions: BTreeSet<ContractVersionKey>,
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

    /// Enable the contract version corresponding to the given hash (if it exists).
    pub fn enable_version(&mut self, contract_hash: AddressableEntityHash) -> Result<(), Error> {
        let contract_version_key = self
            .find_contract_version_key_by_hash(&contract_hash)
            .copied()
            .ok_or(Error::ContractNotFound)?;

        self.disabled_versions.remove(&contract_version_key);

        Ok(())
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
        let v = self.groups.0.entry(group).or_insert_with(Default::default);
        v.extend(urefs)
    }

    /// Lookup the contract hash for a given contract version (if present)
    pub fn lookup_contract_hash(
        &self,
        contract_version_key: ContractVersionKey,
    ) -> Option<&AddressableEntityHash> {
        if !self.is_version_enabled(contract_version_key) {
            return None;
        }
        self.versions.0.get(&contract_version_key)
    }

    /// Checks if the given contract version exists and is available for use.
    pub fn is_version_enabled(&self, contract_version_key: ContractVersionKey) -> bool {
        !self.disabled_versions.contains(&contract_version_key)
            && self.versions.0.contains_key(&contract_version_key)
    }

    /// Returns `true` if the given contract hash exists and is enabled.
    pub fn is_contract_enabled(&self, contract_hash: &AddressableEntityHash) -> bool {
        match self.find_contract_version_key_by_hash(contract_hash) {
            Some(version_key) => !self.disabled_versions.contains(version_key),
            None => false,
        }
    }

    /// Insert a new contract version; the next sequential version number will be issued.
    pub fn insert_contract_version(
        &mut self,
        protocol_version_major: ProtocolVersionMajor,
        contract_hash: AddressableEntityHash,
    ) -> ContractVersionKey {
        let contract_version = self.next_contract_version_for(protocol_version_major);
        let key = ContractVersionKey::new(protocol_version_major, contract_version);
        self.versions.0.insert(key, contract_hash);
        key
    }

    /// Disable the contract version corresponding to the given hash (if it exists).
    pub fn disable_contract_version(
        &mut self,
        contract_hash: AddressableEntityHash,
    ) -> Result<(), Error> {
        let contract_version_key = self
            .versions
            .0
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
        contract_hash: &AddressableEntityHash,
    ) -> Option<&ContractVersionKey> {
        self.versions
            .0
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
        for version in &self.versions.0 {
            if !self.is_version_enabled(*version.0) {
                continue;
            }
            ret.0.insert(*version.0, *version.1);
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
    pub fn disabled_versions(&self) -> &BTreeSet<ContractVersionKey> {
        &self.disabled_versions
    }

    /// Returns mut reference to all of this contract's disabled versions.
    pub fn disabled_versions_mut(&mut self) -> &mut BTreeSet<ContractVersionKey> {
        &mut self.disabled_versions
    }

    /// Removes a group from this contract (if it exists).
    pub fn remove_group(&mut self, group: &Group) -> bool {
        self.groups.0.remove(group).is_some()
    }

    /// Gets the next available contract version for the given protocol version
    fn next_contract_version_for(&self, protocol_version: ProtocolVersionMajor) -> ContractVersion {
        let current_version = self
            .versions
            .0
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
        self.enabled_versions().0.keys().next_back().copied()
    }

    /// Return the contract hash for the newest enabled contract version.
    pub fn current_contract_hash(&self) -> Option<AddressableEntityHash> {
        self.enabled_versions().0.values().next_back().copied()
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
    pub fn get_package_kind(&self) -> ContractPackageKind {
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
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
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
        let (disabled_versions, bytes) = BTreeSet::<ContractVersionKey>::from_bytes(bytes)?;
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

#[cfg(test)]
mod tests {
    use core::iter::FromIterator;

    use super::*;
    use crate::{
        AccessRights, ContractVersionKey, EntryPoint, EntryPointAccess, EntryPointType, Parameter,
        ProtocolVersion, URef,
    };
    use alloc::borrow::ToOwned;

    const CONTRACT_HASH_V1: AddressableEntityHash = AddressableEntityHash::new([42; 32]);
    const CONTRACT_HASH_V2: AddressableEntityHash = AddressableEntityHash::new([84; 32]);

    fn make_contract_package_with_two_versions() -> Package {
        let mut contract_package = Package::new(
            URef::new([0; 32], AccessRights::NONE),
            ContractVersions::default(),
            BTreeSet::new(),
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

        let protocol_version = ProtocolVersion::V1_0_0;

        let v1 = contract_package
            .insert_contract_version(protocol_version.value().major, CONTRACT_HASH_V1);
        let v2 = contract_package
            .insert_contract_version(protocol_version.value().major, CONTRACT_HASH_V2);
        assert!(v2 > v1);

        contract_package
    }

    #[test]
    fn next_contract_version() {
        let major = 1;
        let mut contract_package = Package::new(
            URef::new([0; 32], AccessRights::NONE),
            ContractVersions::default(),
            BTreeSet::default(),
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
        let contract_package = make_contract_package_with_two_versions();
        let bytes = contract_package.to_bytes().expect("should serialize");
        let (decoded_package, rem) = Package::from_bytes(&bytes).expect("should deserialize");
        assert_eq!(contract_package, decoded_package);
        assert_eq!(rem.len(), 0);
    }

    #[test]
    fn should_remove_group() {
        let mut contract_package = make_contract_package_with_two_versions();

        assert!(!contract_package.remove_group(&Group::new("Non-existent group")));
        assert!(contract_package.remove_group(&Group::new("Group 1")));
        assert!(!contract_package.remove_group(&Group::new("Group 1"))); // Group no longer exists
    }

    #[test]
    fn should_disable_and_enable_contract_version() {
        const CONTRACT_HASH: AddressableEntityHash = AddressableEntityHash::new([123; 32]);

        let mut contract_package = make_contract_package_with_two_versions();

        assert!(
            !contract_package.is_contract_enabled(&CONTRACT_HASH),
            "nonexisting contract contract should return false"
        );

        assert_eq!(
            contract_package.current_contract_version(),
            Some(ContractVersionKey::new(1, 2))
        );
        assert_eq!(
            contract_package.current_contract_hash(),
            Some(CONTRACT_HASH_V2)
        );

        assert_eq!(
            contract_package.versions(),
            &ContractVersions::from(BTreeMap::from_iter([
                (ContractVersionKey::new(1, 1), CONTRACT_HASH_V1),
                (ContractVersionKey::new(1, 2), CONTRACT_HASH_V2)
            ])),
        );
        assert_eq!(
            contract_package.enabled_versions(),
            ContractVersions::from(BTreeMap::from_iter([
                (ContractVersionKey::new(1, 1), CONTRACT_HASH_V1),
                (ContractVersionKey::new(1, 2), CONTRACT_HASH_V2)
            ])),
        );

        assert!(!contract_package.is_contract_enabled(&CONTRACT_HASH));

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
        assert!(contract_package.is_contract_enabled(&CONTRACT_HASH));

        assert!(
            contract_package.is_contract_enabled(&CONTRACT_HASH),
            "contract should be enabled"
        );

        assert_eq!(
            contract_package.disable_contract_version(CONTRACT_HASH),
            Ok(()),
            "should be able to disable version"
        );
        assert!(!contract_package.is_contract_enabled(&CONTRACT_HASH));

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

        assert_eq!(
            contract_package.current_contract_version(),
            Some(ContractVersionKey::new(1, 2))
        );
        assert_eq!(
            contract_package.current_contract_hash(),
            Some(CONTRACT_HASH_V2)
        );
        assert_eq!(
            contract_package.versions(),
            &ContractVersions::from(BTreeMap::from_iter([
                (ContractVersionKey::new(1, 1), CONTRACT_HASH_V1),
                (ContractVersionKey::new(1, 2), CONTRACT_HASH_V2),
                (next_version, CONTRACT_HASH),
            ])),
        );
        assert_eq!(
            contract_package.enabled_versions(),
            ContractVersions::from(BTreeMap::from_iter([
                (ContractVersionKey::new(1, 1), CONTRACT_HASH_V1),
                (ContractVersionKey::new(1, 2), CONTRACT_HASH_V2),
            ])),
        );
        assert_eq!(
            contract_package.disabled_versions(),
            &BTreeSet::from_iter([next_version]),
        );

        assert_eq!(
            contract_package.current_contract_version(),
            Some(ContractVersionKey::new(1, 2))
        );
        assert_eq!(
            contract_package.current_contract_hash(),
            Some(CONTRACT_HASH_V2)
        );

        assert_eq!(
            contract_package.disable_contract_version(CONTRACT_HASH_V2),
            Ok(()),
            "should be able to disable version 2"
        );

        assert_eq!(
            contract_package.enabled_versions(),
            ContractVersions::from(BTreeMap::from_iter([(
                ContractVersionKey::new(1, 1),
                CONTRACT_HASH_V1
            ),])),
        );

        assert_eq!(
            contract_package.current_contract_version(),
            Some(ContractVersionKey::new(1, 1))
        );
        assert_eq!(
            contract_package.current_contract_hash(),
            Some(CONTRACT_HASH_V1)
        );

        assert_eq!(
            contract_package.disabled_versions(),
            &BTreeSet::from_iter([next_version, ContractVersionKey::new(1, 2)]),
        );

        assert_eq!(contract_package.enable_version(CONTRACT_HASH_V2), Ok(()),);

        assert_eq!(
            contract_package.enabled_versions(),
            ContractVersions::from(BTreeMap::from_iter([
                (ContractVersionKey::new(1, 1), CONTRACT_HASH_V1),
                (ContractVersionKey::new(1, 2), CONTRACT_HASH_V2),
            ])),
        );

        assert_eq!(
            contract_package.disabled_versions(),
            &BTreeSet::from_iter([next_version])
        );

        assert_eq!(
            contract_package.current_contract_hash(),
            Some(CONTRACT_HASH_V2)
        );

        assert_eq!(contract_package.enable_version(CONTRACT_HASH), Ok(()),);

        assert_eq!(
            contract_package.enable_version(CONTRACT_HASH),
            Ok(()),
            "enabling a contract twice should be a noop"
        );

        assert_eq!(
            contract_package.enabled_versions(),
            ContractVersions::from(BTreeMap::from_iter([
                (ContractVersionKey::new(1, 1), CONTRACT_HASH_V1),
                (ContractVersionKey::new(1, 2), CONTRACT_HASH_V2),
                (next_version, CONTRACT_HASH),
            ])),
        );

        assert_eq!(contract_package.disabled_versions(), &BTreeSet::new(),);

        assert_eq!(
            contract_package.current_contract_hash(),
            Some(CONTRACT_HASH)
        );
    }

    #[test]
    fn should_not_allow_to_enable_non_existing_version() {
        let mut contract_package = make_contract_package_with_two_versions();

        assert_eq!(
            contract_package.enable_version(AddressableEntityHash::default()),
            Err(Error::ContractNotFound),
        );
    }

    #[test]
    fn contract_package_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash = HashAddr::try_from(&bytes[..]).expect("should create contract hash");
        let contract_hash = ContractPackageHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_package_hash_from_str() {
        let contract_package_hash = ContractPackageHash::new([3; 32]);
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
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_contract_package(contract_pkg in gens::contract_package_arb()) {
            bytesrepr::test_serialization_roundtrip(&contract_pkg);
        }
    }
}
