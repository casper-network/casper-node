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
    addressable_entity::{Error, FromStrError},
    bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH},
    checksummed_hex,
    crypto::{self, PublicKey},
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
pub type EntityVersion = u32;

/// Within each discrete major `ProtocolVersion`, entity version resets to this value.
pub const ENTITY_INITIAL_VERSION: EntityVersion = 1;

/// Major element of `ProtocolVersion` a `EntityVersion` is compatible with.
pub type ProtocolVersionMajor = u32;

/// Major element of `ProtocolVersion` combined with `EntityVersion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EntityVersionKey {
    /// Major element of `ProtocolVersion` a `ContractVersion` is compatible with.
    protocol_version_major: ProtocolVersionMajor,
    /// Automatically incremented value for a contract version within a major `ProtocolVersion`.
    entity_version: EntityVersion,
}

impl EntityVersionKey {
    /// Returns a new instance of ContractVersionKey with provided values.
    pub fn new(
        protocol_version_major: ProtocolVersionMajor,
        entity_version: EntityVersion,
    ) -> Self {
        Self {
            protocol_version_major,
            entity_version,
        }
    }

    /// Returns the major element of the protocol version this contract is compatible with.
    pub fn protocol_version_major(self) -> ProtocolVersionMajor {
        self.protocol_version_major
    }

    /// Returns the contract version within the protocol major version.
    pub fn entity_version(self) -> EntityVersion {
        self.entity_version
    }
}

impl From<EntityVersionKey> for (ProtocolVersionMajor, EntityVersion) {
    fn from(entity_version_key: EntityVersionKey) -> Self {
        (
            entity_version_key.protocol_version_major,
            entity_version_key.entity_version,
        )
    }
}

impl ToBytes for EntityVersionKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        ENTITY_VERSION_KEY_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.protocol_version_major.write_bytes(writer)?;
        self.entity_version.write_bytes(writer)
    }
}

impl FromBytes for EntityVersionKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version_major, remainder) = ProtocolVersionMajor::from_bytes(bytes)?;
        let (entity_version, remainder) = EntityVersion::from_bytes(remainder)?;
        Ok((
            EntityVersionKey {
                protocol_version_major,
                entity_version,
            },
            remainder,
        ))
    }
}

impl Display for EntityVersionKey {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.protocol_version_major, self.entity_version)
    }
}

/// Serialized length of `EntityVersionKey`.
pub const ENTITY_VERSION_KEY_SERIALIZED_LENGTH: usize =
    U32_SERIALIZED_LENGTH + U32_SERIALIZED_LENGTH;

/// Collection of entity versions.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(transparent, deny_unknown_fields)]
pub struct EntityVersions(
    #[serde(
        with = "BTreeMapToArray::<EntityVersionKey, AddressableEntityHash, EntityVersionLabels>"
    )]
    BTreeMap<EntityVersionKey, AddressableEntityHash>,
);

impl EntityVersions {
    /// Constructs a new, empty `EntityVersions`.
    pub const fn new() -> Self {
        EntityVersions(BTreeMap::new())
    }

    /// Returns an iterator over the `AddressableEntityHash`s (i.e. the map's values).
    pub fn contract_hashes(&self) -> impl Iterator<Item = &AddressableEntityHash> {
        self.0.values()
    }

    /// Returns the `AddressableEntityHash` under the key
    pub fn get(&self, key: &EntityVersionKey) -> Option<&AddressableEntityHash> {
        self.0.get(key)
    }

    /// Retrieve the first entity version key if it exists
    pub fn maybe_first(&mut self) -> Option<(EntityVersionKey, AddressableEntityHash)> {
        if let Some((entity_version_key, entity_hash)) = self.0.iter().next() {
            Some((*entity_version_key, *entity_hash))
        } else {
            None
        }
    }
}

impl ToBytes for EntityVersions {
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

impl FromBytes for EntityVersions {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (versions, remainder) =
            BTreeMap::<EntityVersionKey, AddressableEntityHash>::from_bytes(bytes)?;
        Ok((EntityVersions(versions), remainder))
    }
}

impl From<BTreeMap<EntityVersionKey, AddressableEntityHash>> for EntityVersions {
    fn from(value: BTreeMap<EntityVersionKey, AddressableEntityHash>) -> Self {
        EntityVersions(value)
    }
}

struct EntityVersionLabels;

impl KeyValueLabels for EntityVersionLabels {
    const KEY: &'static str = "entity_version_key";
    const VALUE: &'static str = "addressable_entity_hash";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for EntityVersionLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("EntityVersionAndHash");
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
    schemars(description = "The hex-encoded address of the Package.")
)]
pub struct PackageHash(
    #[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))] HashAddr,
);

impl PackageHash {
    /// Constructs a new `PackageHash` from the raw bytes of the package hash.
    pub const fn new(value: HashAddr) -> PackageHash {
        PackageHash(value)
    }

    /// Returns the raw bytes of the entity hash as an array.
    pub fn value(&self) -> HashAddr {
        self.0
    }

    /// Returns the raw bytes of the entity hash as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `PackageHash` for users getting and putting.
    pub fn to_formatted_string(self) -> String {
        format!("{}{}", PACKAGE_STRING_PREFIX, base16::encode_lower(&self.0),)
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a
    /// `PackageHash`.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(PACKAGE_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;

        let hex_addr = remainder
            .strip_prefix(PACKAGE_STRING_LEGACY_EXTRA_PREFIX)
            .unwrap_or(remainder);

        let bytes = HashAddr::try_from(checksummed_hex::decode(hex_addr)?.as_ref())?;
        Ok(PackageHash(bytes))
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

impl Display for PackageHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for PackageHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "PackageHash({})", base16::encode_lower(&self.0))
    }
}

impl CLTyped for PackageHash {
    fn cl_type() -> CLType {
        CLType::ByteArray(KEY_HASH_LENGTH as u32)
    }
}

impl ToBytes for PackageHash {
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

impl FromBytes for PackageHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((PackageHash::new(bytes), rem))
    }
}

impl From<[u8; 32]> for PackageHash {
    fn from(bytes: [u8; 32]) -> Self {
        PackageHash(bytes)
    }
}

impl Serialize for PackageHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for PackageHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            PackageHash::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = HashAddr::deserialize(deserializer)?;
            Ok(PackageHash(bytes))
        }
    }
}

impl AsRef<[u8]> for PackageHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TryFrom<&[u8]> for PackageHash {
    type Error = TryFromSliceForPackageHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForPackageHashError> {
        HashAddr::try_from(bytes)
            .map(PackageHash::new)
            .map_err(|_| TryFromSliceForPackageHashError(()))
    }
}

impl TryFrom<&Vec<u8>> for PackageHash {
    type Error = TryFromSliceForPackageHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        HashAddr::try_from(bytes as &[u8])
            .map(PackageHash::new)
            .map_err(|_| TryFromSliceForPackageHashError(()))
    }
}

impl From<&PublicKey> for PackageHash {
    fn from(public_key: &PublicKey) -> Self {
        PackageHash::from_public_key(public_key, crypto::blake2b)
    }
}

/// A enum to determine the lock status of the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum PackageStatus {
    /// The package is locked and cannot be versioned.
    Locked,
    /// The package is unlocked and can be versioned.
    Unlocked,
}

impl PackageStatus {
    /// Create a new status flag based on a boolean value
    pub fn new(is_locked: bool) -> Self {
        if is_locked {
            PackageStatus::Locked
        } else {
            PackageStatus::Unlocked
        }
    }
}

impl Default for PackageStatus {
    fn default() -> Self {
        Self::Unlocked
    }
}

impl ToBytes for PackageStatus {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        match self {
            PackageStatus::Unlocked => result.append(&mut false.to_bytes()?),
            PackageStatus::Locked => result.append(&mut true.to_bytes()?),
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        match self {
            PackageStatus::Unlocked => false.serialized_length(),
            PackageStatus::Locked => true.serialized_length(),
        }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PackageStatus::Locked => writer.push(u8::from(true)),
            PackageStatus::Unlocked => writer.push(u8::from(false)),
        }
        Ok(())
    }
}

impl FromBytes for PackageStatus {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (val, bytes) = bool::from_bytes(bytes)?;
        let status = PackageStatus::new(val);
        Ok((status, bytes))
    }
}

/// Entity definition, metadata, and security container.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Package {
    /// Key used to add or disable versions.
    access_key: URef,
    /// All versions (enabled & disabled).
    versions: EntityVersions,
    /// Collection of disabled entity versions. The runtime will not permit disabled entity
    /// versions to be executed.
    disabled_versions: BTreeSet<EntityVersionKey>,
    /// Mapping maintaining the set of URefs associated with each "user group". This can be used to
    /// control access to methods in a particular version of the entity. A method is callable by
    /// any context which "knows" any of the URefs associated with the method's user group.
    groups: Groups,
    /// A flag that determines whether a entity is locked
    lock_status: PackageStatus,
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
        versions: EntityVersions,
        disabled_versions: BTreeSet<EntityVersionKey>,
        groups: Groups,
        lock_status: PackageStatus,
    ) -> Self {
        Package {
            access_key,
            versions,
            disabled_versions,
            groups,
            lock_status,
        }
    }

    /// Enable the entity version corresponding to the given hash (if it exists).
    pub fn enable_version(&mut self, entity_hash: AddressableEntityHash) -> Result<(), Error> {
        let entity_version_key = self
            .find_entity_version_key_by_hash(&entity_hash)
            .copied()
            .ok_or(Error::EntityNotFound)?;

        self.disabled_versions.remove(&entity_version_key);

        Ok(())
    }

    /// Get the access key for this entity.
    pub fn access_key(&self) -> URef {
        self.access_key
    }

    /// Get the mutable group definitions for this entity.
    pub fn groups_mut(&mut self) -> &mut Groups {
        &mut self.groups
    }

    /// Get the group definitions for this entity.
    pub fn groups(&self) -> &Groups {
        &self.groups
    }

    /// Adds new group to this entity.
    pub fn add_group(&mut self, group: Group, urefs: BTreeSet<URef>) {
        let v = self.groups.0.entry(group).or_insert_with(Default::default);
        v.extend(urefs)
    }

    /// Lookup the entity hash for a given entity version (if present)
    pub fn lookup_entity_hash(
        &self,
        entity_version_key: EntityVersionKey,
    ) -> Option<&AddressableEntityHash> {
        if !self.is_version_enabled(entity_version_key) {
            return None;
        }
        self.versions.0.get(&entity_version_key)
    }

    /// Checks if the given entity version exists and is available for use.
    pub fn is_version_enabled(&self, entity_version_key: EntityVersionKey) -> bool {
        !self.disabled_versions.contains(&entity_version_key)
            && self.versions.0.contains_key(&entity_version_key)
    }

    /// Returns `true` if the given entity hash exists and is enabled.
    pub fn is_entity_enabled(&self, entity_hash: &AddressableEntityHash) -> bool {
        match self.find_entity_version_key_by_hash(entity_hash) {
            Some(version_key) => !self.disabled_versions.contains(version_key),
            None => false,
        }
    }

    /// Insert a new entity version; the next sequential version number will be issued.
    pub fn insert_entity_version(
        &mut self,
        protocol_version_major: ProtocolVersionMajor,
        entity_hash: AddressableEntityHash,
    ) -> EntityVersionKey {
        let contract_version = self.next_entity_version_for(protocol_version_major);
        let key = EntityVersionKey::new(protocol_version_major, contract_version);
        self.versions.0.insert(key, entity_hash);
        key
    }

    /// Disable the entity version corresponding to the given hash (if it exists).
    pub fn disable_entity_version(
        &mut self,
        entity_hash: AddressableEntityHash,
    ) -> Result<(), Error> {
        let entity_version_key = self
            .versions
            .0
            .iter()
            .filter_map(|(k, v)| if *v == entity_hash { Some(*k) } else { None })
            .next()
            .ok_or(Error::EntityNotFound)?;

        if !self.disabled_versions.contains(&entity_version_key) {
            self.disabled_versions.insert(entity_version_key);
        }

        Ok(())
    }

    fn find_entity_version_key_by_hash(
        &self,
        entity_hash: &AddressableEntityHash,
    ) -> Option<&EntityVersionKey> {
        self.versions
            .0
            .iter()
            .filter_map(|(k, v)| if v == entity_hash { Some(k) } else { None })
            .next()
    }

    /// Returns reference to all of this entity's versions.
    pub fn versions(&self) -> &EntityVersions {
        &self.versions
    }

    /// Returns all of this entity's enabled entity versions.
    pub fn enabled_versions(&self) -> EntityVersions {
        let mut ret = EntityVersions::new();
        for version in &self.versions.0 {
            if !self.is_version_enabled(*version.0) {
                continue;
            }
            ret.0.insert(*version.0, *version.1);
        }
        ret
    }

    /// Returns mutable reference to all of this entity's versions (enabled and disabled).
    pub fn versions_mut(&mut self) -> &mut EntityVersions {
        &mut self.versions
    }

    /// Consumes the object and returns all of this entity's versions (enabled and disabled).
    pub fn take_versions(self) -> EntityVersions {
        self.versions
    }

    /// Returns all of this entity's disabled versions.
    pub fn disabled_versions(&self) -> &BTreeSet<EntityVersionKey> {
        &self.disabled_versions
    }

    /// Returns mut reference to all of this entity's disabled versions.
    pub fn disabled_versions_mut(&mut self) -> &mut BTreeSet<EntityVersionKey> {
        &mut self.disabled_versions
    }

    /// Removes a group from this entity (if it exists).
    pub fn remove_group(&mut self, group: &Group) -> bool {
        self.groups.0.remove(group).is_some()
    }

    /// Gets the next available entity version for the given protocol version
    fn next_entity_version_for(&self, protocol_version: ProtocolVersionMajor) -> EntityVersion {
        let current_version = self
            .versions
            .0
            .keys()
            .rev()
            .find_map(|&entity_version_key| {
                if entity_version_key.protocol_version_major() == protocol_version {
                    Some(entity_version_key.entity_version())
                } else {
                    None
                }
            })
            .unwrap_or(0);

        current_version + 1
    }

    /// Return the entity version key for the newest enabled entity version.
    pub fn current_entity_version(&self) -> Option<EntityVersionKey> {
        self.enabled_versions().0.keys().next_back().copied()
    }

    /// Return the entity hash for the newest enabled entity version.
    pub fn current_entity_hash(&self) -> Option<AddressableEntityHash> {
        self.enabled_versions().0.values().next_back().copied()
    }

    /// Return the lock status of the entity package.
    pub fn is_locked(&self) -> bool {
        if self.versions.0.is_empty() {
            return false;
        }

        match self.lock_status {
            PackageStatus::Unlocked => false,
            PackageStatus::Locked => true,
        }
    }

    // TODO: Check the history of this.
    /// Return the package status itself
    pub fn get_lock_status(&self) -> PackageStatus {
        self.lock_status.clone()
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

impl FromBytes for Package {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (access_key, bytes) = URef::from_bytes(bytes)?;
        let (versions, bytes) = EntityVersions::from_bytes(bytes)?;
        let (disabled_versions, bytes) = BTreeSet::<EntityVersionKey>::from_bytes(bytes)?;
        let (groups, bytes) = Groups::from_bytes(bytes)?;
        let (lock_status, bytes) = PackageStatus::from_bytes(bytes)?;

        let result = Package {
            access_key,
            versions,
            disabled_versions,
            groups,
            lock_status,
        };

        Ok((result, bytes))
    }
}

#[cfg(test)]
mod tests {
    use core::iter::FromIterator;

    use super::*;
    use crate::{
        AccessRights, EntityVersionKey, EntryPoint, EntryPointAccess, EntryPointType, Parameter,
        ProtocolVersion, URef,
    };
    use alloc::borrow::ToOwned;

    const ENTITY_HASH_V1: AddressableEntityHash = AddressableEntityHash::new([42; 32]);
    const ENTITY_HASH_V2: AddressableEntityHash = AddressableEntityHash::new([84; 32]);

    fn make_package_with_two_versions() -> Package {
        let mut package = Package::new(
            URef::new([0; 32], AccessRights::NONE),
            EntityVersions::default(),
            BTreeSet::new(),
            Groups::default(),
            PackageStatus::default(),
        );

        // add groups
        {
            let group_urefs = {
                let mut ret = BTreeSet::new();
                ret.insert(URef::new([1; 32], AccessRights::READ));
                ret
            };

            package
                .groups_mut()
                .insert(Group::new("Group 1"), group_urefs.clone());

            package
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

        let v1 = package.insert_entity_version(protocol_version.value().major, ENTITY_HASH_V1);
        let v2 = package.insert_entity_version(protocol_version.value().major, ENTITY_HASH_V2);
        assert!(v2 > v1);

        package
    }

    #[test]
    fn next_entity_version() {
        let major = 1;
        let mut package = Package::new(
            URef::new([0; 32], AccessRights::NONE),
            EntityVersions::default(),
            BTreeSet::default(),
            Groups::default(),
            PackageStatus::default(),
        );
        assert_eq!(package.next_entity_version_for(major), 1);

        let next_version = package.insert_entity_version(major, [123; 32].into());
        assert_eq!(next_version, EntityVersionKey::new(major, 1));
        assert_eq!(package.next_entity_version_for(major), 2);
        let next_version_2 = package.insert_entity_version(major, [124; 32].into());
        assert_eq!(next_version_2, EntityVersionKey::new(major, 2));

        let major = 2;
        assert_eq!(package.next_entity_version_for(major), 1);
        let next_version_3 = package.insert_entity_version(major, [42; 32].into());
        assert_eq!(next_version_3, EntityVersionKey::new(major, 1));
    }

    #[test]
    fn roundtrip_serialization() {
        let package = make_package_with_two_versions();
        let bytes = package.to_bytes().expect("should serialize");
        let (decoded_package, rem) = Package::from_bytes(&bytes).expect("should deserialize");
        assert_eq!(package, decoded_package);
        assert_eq!(rem.len(), 0);
    }

    #[test]
    fn should_remove_group() {
        let mut package = make_package_with_two_versions();

        assert!(!package.remove_group(&Group::new("Non-existent group")));
        assert!(package.remove_group(&Group::new("Group 1")));
        assert!(!package.remove_group(&Group::new("Group 1"))); // Group no longer exists
    }

    #[test]
    fn should_disable_and_enable_entity_version() {
        const ENTITY_HASH: AddressableEntityHash = AddressableEntityHash::new([123; 32]);

        let mut package = make_package_with_two_versions();

        assert!(
            !package.is_entity_enabled(&ENTITY_HASH),
            "nonexisting entity should return false"
        );

        assert_eq!(
            package.current_entity_version(),
            Some(EntityVersionKey::new(1, 2))
        );
        assert_eq!(package.current_entity_hash(), Some(ENTITY_HASH_V2));

        assert_eq!(
            package.versions(),
            &EntityVersions::from(BTreeMap::from_iter([
                (EntityVersionKey::new(1, 1), ENTITY_HASH_V1),
                (EntityVersionKey::new(1, 2), ENTITY_HASH_V2)
            ])),
        );
        assert_eq!(
            package.enabled_versions(),
            EntityVersions::from(BTreeMap::from_iter([
                (EntityVersionKey::new(1, 1), ENTITY_HASH_V1),
                (EntityVersionKey::new(1, 2), ENTITY_HASH_V2)
            ])),
        );

        assert!(!package.is_entity_enabled(&ENTITY_HASH));

        assert_eq!(
            package.disable_entity_version(ENTITY_HASH),
            Err(Error::EntityNotFound),
            "should return entity not found error"
        );

        assert!(
            !package.is_entity_enabled(&ENTITY_HASH),
            "disabling missing entity shouldnt change outcome"
        );

        let next_version = package.insert_entity_version(1, ENTITY_HASH);
        assert!(
            package.is_version_enabled(next_version),
            "version should exist and be enabled"
        );
        assert!(package.is_entity_enabled(&ENTITY_HASH));

        assert!(
            package.is_entity_enabled(&ENTITY_HASH),
            "entity should be enabled"
        );

        assert_eq!(
            package.disable_entity_version(ENTITY_HASH),
            Ok(()),
            "should be able to disable version"
        );
        assert!(!package.is_entity_enabled(&ENTITY_HASH));

        assert!(
            !package.is_entity_enabled(&ENTITY_HASH),
            "entity should be disabled"
        );
        assert_eq!(
            package.lookup_entity_hash(next_version),
            None,
            "should not return disabled entity version"
        );
        assert!(
            !package.is_version_enabled(next_version),
            "version should not be enabled"
        );

        assert_eq!(
            package.current_entity_version(),
            Some(EntityVersionKey::new(1, 2))
        );
        assert_eq!(package.current_entity_hash(), Some(ENTITY_HASH_V2));
        assert_eq!(
            package.versions(),
            &EntityVersions::from(BTreeMap::from_iter([
                (EntityVersionKey::new(1, 1), ENTITY_HASH_V1),
                (EntityVersionKey::new(1, 2), ENTITY_HASH_V2),
                (next_version, ENTITY_HASH),
            ])),
        );
        assert_eq!(
            package.enabled_versions(),
            EntityVersions::from(BTreeMap::from_iter([
                (EntityVersionKey::new(1, 1), ENTITY_HASH_V1),
                (EntityVersionKey::new(1, 2), ENTITY_HASH_V2),
            ])),
        );
        assert_eq!(
            package.disabled_versions(),
            &BTreeSet::from_iter([next_version]),
        );

        assert_eq!(
            package.current_entity_version(),
            Some(EntityVersionKey::new(1, 2))
        );
        assert_eq!(package.current_entity_hash(), Some(ENTITY_HASH_V2));

        assert_eq!(
            package.disable_entity_version(ENTITY_HASH_V2),
            Ok(()),
            "should be able to disable version 2"
        );

        assert_eq!(
            package.enabled_versions(),
            EntityVersions::from(BTreeMap::from_iter([(
                EntityVersionKey::new(1, 1),
                ENTITY_HASH_V1
            ),])),
        );

        assert_eq!(
            package.current_entity_version(),
            Some(EntityVersionKey::new(1, 1))
        );
        assert_eq!(package.current_entity_hash(), Some(ENTITY_HASH_V1));

        assert_eq!(
            package.disabled_versions(),
            &BTreeSet::from_iter([next_version, EntityVersionKey::new(1, 2)]),
        );

        assert_eq!(package.enable_version(ENTITY_HASH_V2), Ok(()),);

        assert_eq!(
            package.enabled_versions(),
            EntityVersions::from(BTreeMap::from_iter([
                (EntityVersionKey::new(1, 1), ENTITY_HASH_V1),
                (EntityVersionKey::new(1, 2), ENTITY_HASH_V2),
            ])),
        );

        assert_eq!(
            package.disabled_versions(),
            &BTreeSet::from_iter([next_version])
        );

        assert_eq!(package.current_entity_hash(), Some(ENTITY_HASH_V2));

        assert_eq!(package.enable_version(ENTITY_HASH), Ok(()),);

        assert_eq!(
            package.enable_version(ENTITY_HASH),
            Ok(()),
            "enabling a entity twice should be a noop"
        );

        assert_eq!(
            package.enabled_versions(),
            EntityVersions::from(BTreeMap::from_iter([
                (EntityVersionKey::new(1, 1), ENTITY_HASH_V1),
                (EntityVersionKey::new(1, 2), ENTITY_HASH_V2),
                (next_version, ENTITY_HASH),
            ])),
        );

        assert_eq!(package.disabled_versions(), &BTreeSet::new(),);

        assert_eq!(package.current_entity_hash(), Some(ENTITY_HASH));
    }

    #[test]
    fn should_not_allow_to_enable_non_existing_version() {
        let mut package = make_package_with_two_versions();

        assert_eq!(
            package.enable_version(AddressableEntityHash::default()),
            Err(Error::EntityNotFound),
        );
    }

    #[test]
    fn package_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let package_hash = HashAddr::try_from(&bytes[..]).expect("should create package hash");
        let package_hash = PackageHash::new(package_hash);
        assert_eq!(&bytes, &package_hash.as_bytes());
    }

    #[test]
    fn package_hash_from_str() {
        let package_hash = PackageHash::new([3; 32]);
        let encoded = package_hash.to_formatted_string();
        let decoded = PackageHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(package_hash, decoded);

        let invalid_prefix =
            "contract-package0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            PackageHash::from_formatted_str(invalid_prefix).unwrap_err(),
            FromStrError::InvalidPrefix
        ));

        let short_addr =
            "contract-package-00000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            PackageHash::from_formatted_str(short_addr).unwrap_err(),
            FromStrError::Hash(_)
        ));

        let long_addr =
            "contract-package-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            PackageHash::from_formatted_str(long_addr).unwrap_err(),
            FromStrError::Hash(_)
        ));

        let invalid_hex =
            "contract-package-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(matches!(
            PackageHash::from_formatted_str(invalid_hex).unwrap_err(),
            FromStrError::Hex(_)
        ));
    }

    #[test]
    fn package_hash_from_legacy_str() {
        let package_hash = PackageHash([3; 32]);
        let hex_addr = package_hash.to_string();
        let legacy_encoded = format!("contract-package-wasm{}", hex_addr);
        let decoded_from_legacy = PackageHash::from_formatted_str(&legacy_encoded)
            .expect("should accept legacy prefixed string");
        assert_eq!(
            package_hash, decoded_from_legacy,
            "decoded_from_legacy should equal decoded"
        );

        let invalid_prefix =
            "contract-packagewasm0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            PackageHash::from_formatted_str(invalid_prefix).unwrap_err(),
            FromStrError::InvalidPrefix
        ));

        let short_addr =
            "contract-package-wasm00000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            PackageHash::from_formatted_str(short_addr).unwrap_err(),
            FromStrError::Hash(_)
        ));

        let long_addr =
            "contract-package-wasm000000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            PackageHash::from_formatted_str(long_addr).unwrap_err(),
            FromStrError::Hash(_)
        ));

        let invalid_hex =
            "contract-package-wasm000000000000000000000000000000000000000000000000000000000000000g";
        assert!(matches!(
            PackageHash::from_formatted_str(invalid_hex).unwrap_err(),
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
        fn test_value_contract_package(contract_pkg in gens::package_arb()) {
            bytesrepr::test_serialization_roundtrip(&contract_pkg);
        }
    }
}
