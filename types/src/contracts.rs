//! Data types for supporting contract headers feature.
use alloc::vec::Vec;
use core::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};
#[cfg(feature = "datasize")]
use datasize::DataSize;

#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    addressable_entity::{EntryPoint, NamedKeys},
    bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH},
    contract_wasm::ContractWasmHash,
    ContractPackageHash, EntryPoints, ProtocolVersion, KEY_HASH_LENGTH,
};

/// Maximum number of distinct user groups.
pub const MAX_GROUPS: u8 = 10;
/// Maximum number of URefs which can be assigned across all user groups.
pub const MAX_TOTAL_UREFS: usize = 100;

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

/// Automatically incremented value for a contract version within a major `ProtocolVersion`.
pub type ContractVersion = u32;

/// Within each discrete major `ProtocolVersion`, contract version resets to this value.
pub const CONTRACT_INITIAL_VERSION: ContractVersion = 1;

/// Major element of `ProtocolVersion` a `ContractVersion` is compatible with.
pub type ProtocolVersionMajor = u32;

/// Serialized length of `ContractVersionKey`.
pub const CONTRACT_VERSION_KEY_SERIALIZED_LENGTH: usize =
    U32_SERIALIZED_LENGTH + U32_SERIALIZED_LENGTH;

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

    /// Appends new named keys.
    pub fn append_named_keys(&mut self, named_keys: NamedKeys) {
        self.named_keys.append(named_keys)
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

impl Default for Contract {
    fn default() -> Self {
        Contract {
            named_keys: NamedKeys::new(),
            entry_points: EntryPoints::new_with_default_entry_point(),
            contract_wasm_hash: [0; KEY_HASH_LENGTH].into(),
            contract_package_hash: [0; KEY_HASH_LENGTH].into(),
            protocol_version: ProtocolVersion::V1_0_0,
        }
    }
}
