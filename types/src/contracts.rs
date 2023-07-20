//! Data types for supporting contract headers feature.
// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::vec::Vec;
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

use crate::addressable_entity::{ActionThresholds, AddressableEntity, AssociatedKeys, NamedKeys};
use crate::package::ContractPackageHash;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH},
    checksummed_hex,
    contract_wasm::ContractWasmHash,
    crypto::{self, PublicKey},
    system::SystemContractType,
    uref,
    uref::URef,
    AccessRights, CLType, CLTyped, ContextAccessRights, EntryPoint, EntryPoints, HashAddr, Key,
    ProtocolVersion, BLAKE2B_DIGEST_LENGTH, KEY_HASH_LENGTH,
};

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
