// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};
use semver::Version;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use casper_types::{
    Contract as DomainContract, ContractHash as DomainHash,
    ContractPackage as DomainContractPackage, ContractPackageHash as DomainPackageHash,
    ContractWasmHash as DomainWasmHash, EntryPoint, NamedKey, URef,
};

use crate::types::json_compatibility::vectorize;

/// A prefix to identify a ContractHash.
const CONTRACT_STRING_PREFIX: &str = "contract-";
/// A prefix to identify a ContractPackageHash.
const PACKAGE_STRING_PREFIX: &str = "contract-package-";
/// A prefix to identify a ContractWasmHash.
const WASM_STRING_PREFIX: &str = "contract-wasm-";

/// Error in parsing a formatted string
#[derive(Debug)]
pub enum FromStrError {
    /// The prefix is invalid.
    InvalidPrefix,
    /// The hash is not valid hex.
    Hex(base16::DecodeError),
    /// The hash is the wrong length
    Hash(TryFromSliceError),
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

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            FromStrError::InvalidPrefix => write!(f, "prefix is not 'contract-' "),
            FromStrError::Hex(error) => {
                write!(f, "failed to decode address portion from hex: {}", error)
            }
            FromStrError::Hash(error) => {
                write!(f, "address portion is wrong length: {}", error)
            }
        }
    }
}

/// A wrapper to help serialize and deserialize ContractHash over JSON.
#[derive(DataSize, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ContractHash(DomainHash);

impl ContractHash {
    /// Construct a new JSON Compatible representation of a ContractHash
    pub const fn new(value: DomainHash) -> ContractHash {
        ContractHash(value)
    }

    /// Returns the value of the underlying contract hash.
    pub fn _value(&self) -> DomainHash {
        self.0
    }

    /// Returns the underlying raw bytes of the contract hash a `slice`
    #[cfg(test)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `ContractHash` for users getting and putting.
    pub fn to_formatted_string(&self) -> String {
        format!(
            "{}{}",
            CONTRACT_STRING_PREFIX,
            base16::encode_lower(&self.0)
        )
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a `ContractHash`
    pub fn from_formatted_string(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(CONTRACT_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = DomainHash::try_from(base16::decode(remainder)?.as_ref())?;
        Ok(ContractHash(bytes))
    }
}

impl JsonSchema for ContractHash {
    fn schema_name() -> String {
        String::from("ContractHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some("Hex-encoded contract hash".to_string());
        schema_object.into()
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
            ContractHash::from_formatted_string(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = DomainHash::deserialize(deserializer)?;
            Ok(ContractHash(bytes))
        }
    }
}

impl Display for ContractHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for ContractHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "ContractHash({})", base16::encode_lower(&self.0))
    }
}

/// A wrapper to help serialize and deserialize ContractWasmHash over JSON.
#[derive(DataSize, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ContractWasmHash(DomainWasmHash);

impl ContractWasmHash {
    /// Construct a new JSON Compatible representation of a ContractWasmHash
    pub const fn new(value: DomainHash) -> ContractWasmHash {
        ContractWasmHash(value)
    }

    /// Returns the value of the underlying contract hash.
    pub fn _value(&self) -> DomainWasmHash {
        self.0
    }

    /// Returns the underlying raw bytes of the contract hash a `slice`
    #[cfg(test)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `ContractHash` for users getting and putting.
    pub fn to_formatted_string(&self) -> String {
        format!("{}{}", WASM_STRING_PREFIX, base16::encode_lower(&self.0))
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a `ContractWasmHash`
    pub fn from_formatted_string(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(WASM_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = DomainHash::try_from(base16::decode(remainder)?.as_ref())?;
        Ok(ContractWasmHash(bytes))
    }
}

impl JsonSchema for ContractWasmHash {
    fn schema_name() -> String {
        String::from("ContractWasmHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some("Hex-encoded contract wasm hash".to_string());
        schema_object.into()
    }
}

impl Serialize for ContractWasmHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for ContractWasmHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            ContractWasmHash::from_formatted_string(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = DomainWasmHash::deserialize(deserializer)?;
            Ok(ContractWasmHash(bytes))
        }
    }
}

impl Display for ContractWasmHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for ContractWasmHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "ContractWasmHash({})", base16::encode_lower(&self.0))
    }
}

/// A wrapper to help serialize and deserialize ContractHash over JSON.
#[derive(DataSize, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ContractPackageHash(DomainPackageHash);

impl ContractPackageHash {
    /// Construct a new JSON Compatible representation of a ContractHash
    pub const fn new(value: DomainHash) -> ContractPackageHash {
        ContractPackageHash(value)
    }

    /// Returns the value of the underlying contract hash.
    pub fn _value(&self) -> DomainPackageHash {
        self.0
    }

    /// Returns the underlying raw bytes of the contract hash a `slice`
    #[cfg(test)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `ContractPackageHash` for users getting and putting.
    pub fn to_formatted_string(&self) -> String {
        format!("{}{}", PACKAGE_STRING_PREFIX, base16::encode_lower(&self.0))
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a `ContractPackageHash`
    pub fn from_formatted_string(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(PACKAGE_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = DomainPackageHash::try_from(base16::decode(remainder)?.as_ref())?;
        Ok(ContractPackageHash(bytes))
    }
}

impl JsonSchema for ContractPackageHash {
    fn schema_name() -> String {
        String::from("ContractPackageHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some("Hex-encoded contract hash".to_string());
        schema_object.into()
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
            ContractPackageHash::from_formatted_string(&formatted_string)
                .map_err(SerdeError::custom)
        } else {
            let bytes = DomainPackageHash::deserialize(deserializer)?;
            Ok(ContractPackageHash(bytes))
        }
    }
}

impl Display for ContractPackageHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for ContractPackageHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "ContractPackageHash({})", base16::encode_lower(&self.0))
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, DataSize, JsonSchema,
)]
pub struct ContractVersion {
    protocol_version_major: u32,
    contract_version: u32,
    contract_hash: ContractHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, JsonSchema)]
pub struct DisabledVersion {
    protocol_version_major: u32,
    contract_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, DataSize, JsonSchema)]
pub struct Groups {
    group: String,
    #[data_size(skip)]
    keys: Vec<URef>,
}

/// A contract struct that can be serialized as  JSON object.
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, DataSize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    contract_package_hash: ContractPackageHash,
    contract_wasm_hash: ContractWasmHash,
    #[data_size(skip)]
    named_keys: Vec<NamedKey>,
    #[data_size(skip)]
    entry_points: Vec<EntryPoint>,
    #[data_size(skip)]
    #[schemars(with = "String")]
    protocol_version: Version,
}

impl From<&DomainContract> for Contract {
    fn from(contract: &DomainContract) -> Self {
        let entry_points = contract.entry_points().clone().take_entry_points();
        let named_keys = vectorize(contract.named_keys());
        let protocol_version = Version::from((
            contract.protocol_version().value().major as u64,
            contract.protocol_version().value().minor as u64,
            contract.protocol_version().value().patch as u64,
        ));

        Contract {
            contract_package_hash: ContractPackageHash::new(contract.contract_package_hash()),
            contract_wasm_hash: ContractWasmHash::new(contract.contract_wasm_hash()),
            named_keys,
            entry_points,
            protocol_version,
        }
    }
}

/// Contract definition, metadata, and security container.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, DataSize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContractPackage {
    #[data_size(skip)]
    access_key: URef,
    versions: Vec<ContractVersion>,
    disabled_versions: Vec<DisabledVersion>,
    groups: Vec<Groups>,
}

impl From<&DomainContractPackage> for ContractPackage {
    fn from(contract_package: &DomainContractPackage) -> Self {
        let versions = contract_package
            .versions()
            .iter()
            .map(|(version_key, hash)| ContractVersion {
                protocol_version_major: version_key.protocol_version_major(),
                contract_version: version_key.contract_version(),
                contract_hash: ContractHash::new(*hash),
            })
            .collect();

        let disabled_versions = contract_package
            .disabled_versions()
            .iter()
            .map(|version| DisabledVersion {
                protocol_version_major: version.protocol_version_major(),
                contract_version: version.contract_version(),
            })
            .collect();

        let groups = contract_package
            .groups()
            .iter()
            .map(|(group, keys)| Groups {
                group: group.clone().value().to_string(),
                keys: keys.iter().cloned().collect(),
            })
            .collect();

        ContractPackage {
            access_key: contract_package.access_key(),
            versions,
            disabled_versions,
            groups,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::TryFrom, vec::Vec};

    use super::*;

    #[test]
    fn contract_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash = DomainHash::try_from(&bytes[..]).expect("should create contract hash");
        let contract_hash = ContractHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_wasm_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash =
            DomainWasmHash::try_from(&bytes[..]).expect("should create contract wasm hash");
        let contract_hash = ContractWasmHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_package_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash =
            DomainPackageHash::try_from(&bytes[..]).expect("should create contract hash");
        let contract_hash = ContractPackageHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_hash_from_str() {
        let contract_hash = ContractHash([3; 32]);
        let encoded = contract_hash.to_formatted_string();
        let decoded = ContractHash::from_formatted_string(&encoded).unwrap();
        assert_eq!(contract_hash, decoded);

        let invalid_prefix =
            "contract--0000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_string(invalid_prefix).is_err());

        let short_addr = "contract-00000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_string(short_addr).is_err());

        let long_addr =
            "contract-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_string(long_addr).is_err());

        let invalid_hex =
            "contract-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(ContractHash::from_formatted_string(invalid_hex).is_err());
    }

    #[test]
    fn contract_wasm_hash_from_str() {
        let contract_hash = ContractWasmHash([3; 32]);
        let encoded = contract_hash.to_formatted_string();
        let decoded = ContractWasmHash::from_formatted_string(&encoded).unwrap();
        assert_eq!(contract_hash, decoded);

        let invalid_prefix =
            "contractwasm-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractWasmHash::from_formatted_string(invalid_prefix).is_err());

        let short_addr =
            "contract-wasm-00000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractWasmHash::from_formatted_string(short_addr).is_err());

        let long_addr =
            "contract-wasm-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractWasmHash::from_formatted_string(long_addr).is_err());

        let invalid_hex =
            "contract-wasm-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(ContractWasmHash::from_formatted_string(invalid_hex).is_err());
    }

    #[test]
    fn contract_package_hash_from_str() {
        let contract_hash = ContractPackageHash([3; 32]);
        let encoded = contract_hash.to_formatted_string();
        let decoded = ContractPackageHash::from_formatted_string(&encoded).unwrap();
        assert_eq!(contract_hash, decoded);

        let invalid_prefix =
            "contractpackage-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractPackageHash::from_formatted_string(invalid_prefix).is_err());

        let short_addr =
            "contract-package-00000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractPackageHash::from_formatted_string(short_addr).is_err());

        let long_addr =
            "contract-package-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractPackageHash::from_formatted_string(long_addr).is_err());

        let invalid_hex =
            "contract-package-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(ContractPackageHash::from_formatted_string(invalid_hex).is_err());
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
    fn contract_wasm_hash_serde_roundtrip() {
        let contract_hash = ContractWasmHash([255; 32]);
        let serialized = bincode::serialize(&contract_hash).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(contract_hash, deserialized)
    }

    #[test]
    fn contract_wasm_hash_json_roundtrip() {
        let contract_hash = ContractWasmHash([255; 32]);
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
