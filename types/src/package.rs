//! Module containing the Package and associated types for addressable entities.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    account::AccountHash,
    addressable_entity::{AssociatedKeys, Error, FromStrError, Weight},
    bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH},
    checksummed_hex,
    crypto::{self, PublicKey},
    system::SystemContractType,
    uref::URef,
    CLType, CLTyped, ContractHash, HashAddr, BLAKE2B_DIGEST_LENGTH, KEY_HASH_LENGTH,
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

    /// Returns if the current package is either a system contract or the system entity.
    pub fn is_system(&self) -> bool {
        match self {
            ContractPackageKind::System(_) => true,
            _ => false,
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        addressable_entity::NamedKeys,
        package::{
            ContractPackageKind, ContractPackageStatus, ContractVersions, DisabledVersions, Groups,
            Package,
        },
        AccessRights, ContractVersionKey, EntryPoint, EntryPointAccess, EntryPointType, Parameter,
        ProtocolVersion, URef,
    };
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
        // #![proptest_config(ProptestConfig {
        //     cases: 1024,
        //     .. ProptestConfig::default()
        // })]



        #[test]
        fn test_value_contract_package(contract_pkg in gens::contract_package_arb()) {
            bytesrepr::test_serialization_roundtrip(&contract_pkg);
        }
    }
}
