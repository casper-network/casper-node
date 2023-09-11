use alloc::{format, string::String, vec::Vec};
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
    addressable_entity,
    bytesrepr::{Bytes, Error, FromBytes, ToBytes},
    checksummed_hex, uref, CLType, CLTyped, HashAddr,
};

const BYTE_CODE_MAX_DISPLAY_LEN: usize = 16;
const KEY_HASH_LENGTH: usize = 32;
const WASM_STRING_PREFIX: &str = "contract-wasm-";

/// Associated error type of `TryFrom<&[u8]>` for `ByteCodeHash`.
#[derive(Debug)]
pub struct TryFromSliceForContractHashError(());

#[derive(Debug)]
#[non_exhaustive]
pub enum FromStrError {
    InvalidPrefix,
    Hex(base16::DecodeError),
    Hash(TryFromSliceError),
    AccountHash(addressable_entity::FromAccountHashStrError),
    URef(uref::FromStrError),
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

impl From<addressable_entity::FromAccountHashStrError> for FromStrError {
    fn from(error: addressable_entity::FromAccountHashStrError) -> Self {
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
            FromStrError::Hash(error) => write!(f, "hash from string error: {}", error),
            FromStrError::AccountHash(error) => {
                write!(f, "account hash from string error: {:?}", error)
            }
            FromStrError::URef(error) => write!(f, "uref from string error: {:?}", error),
        }
    }
}

/// A newtype wrapping a `HashAddr` which is the raw bytes of
/// the ByteCodeHash
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ByteCodeHash(HashAddr);

impl ByteCodeHash {
    /// Constructs a new `ContractWasmHash` from the raw bytes of the contract wasm hash.
    pub const fn new(value: HashAddr) -> ByteCodeHash {
        ByteCodeHash(value)
    }

    /// Returns the raw bytes of the contract hash as an array.
    pub fn value(&self) -> HashAddr {
        self.0
    }

    /// Returns the raw bytes of the contract hash as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `ContractWasmHash` for users getting and putting.
    pub fn to_formatted_string(self) -> String {
        format!("{}{}", WASM_STRING_PREFIX, base16::encode_lower(&self.0),)
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a
    /// `ByteCodeHash`.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(WASM_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = HashAddr::try_from(checksummed_hex::decode(remainder)?.as_ref())?;
        Ok(ByteCodeHash(bytes))
    }
}

impl Display for ByteCodeHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for ByteCodeHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "ByteCodeHash({})", base16::encode_lower(&self.0))
    }
}

impl CLTyped for ByteCodeHash {
    fn cl_type() -> CLType {
        CLType::ByteArray(KEY_HASH_LENGTH as u32)
    }
}

impl ToBytes for ByteCodeHash {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    #[inline(always)]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.0.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for ByteCodeHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((ByteCodeHash::new(bytes), rem))
    }
}

impl From<[u8; 32]> for ByteCodeHash {
    fn from(bytes: [u8; 32]) -> Self {
        ByteCodeHash(bytes)
    }
}

impl Serialize for ByteCodeHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for ByteCodeHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            ByteCodeHash::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = HashAddr::deserialize(deserializer)?;
            Ok(ByteCodeHash(bytes))
        }
    }
}

impl AsRef<[u8]> for ByteCodeHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TryFrom<&[u8]> for ByteCodeHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForContractHashError> {
        HashAddr::try_from(bytes)
            .map(ByteCodeHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

impl TryFrom<&Vec<u8>> for ByteCodeHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        HashAddr::try_from(bytes as &[u8])
            .map(ByteCodeHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

#[cfg(feature = "json-schema")]
impl JsonSchema for ByteCodeHash {
    fn schema_name() -> String {
        String::from("ContractWasmHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description =
            Some("The hash address of the contract wasm".to_string());
        schema_object.into()
    }
}

/// A container for contract's WASM bytes.
#[derive(PartialEq, Eq, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct ByteCode {
    bytes: Bytes,
}

impl Debug for ByteCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.bytes.len() > BYTE_CODE_MAX_DISPLAY_LEN {
            write!(
                f,
                "ContractWasm(0x{}...)",
                base16::encode_lower(&self.bytes[..BYTE_CODE_MAX_DISPLAY_LEN])
            )
        } else {
            write!(f, "ContractWasm(0x{})", base16::encode_lower(&self.bytes))
        }
    }
}

impl ByteCode {
    /// Creates new WASM object from bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        ByteCode {
            bytes: bytes.into(),
        }
    }

    /// Consumes instance of [`ByteCode`] and returns its bytes.
    pub fn take_bytes(self) -> Vec<u8> {
        self.bytes.into()
    }

    /// Returns a slice of contained WASM bytes.
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

impl ToBytes for ByteCode {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.bytes.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.bytes.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.bytes.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for ByteCode {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem1) = FromBytes::from_bytes(bytes)?;
        Ok((ByteCode { bytes }, rem1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_debug_repr_of_short_wasm() {
        const SIZE: usize = 8;
        let wasm_bytes = vec![0; SIZE];
        let contract_wasm = ByteCode::new(wasm_bytes);
        // String output is less than the bytes itself
        assert_eq!(
            format!("{:?}", contract_wasm),
            "ContractWasm(0x0000000000000000)"
        );
    }

    #[test]
    fn test_debug_repr_of_long_wasm() {
        const SIZE: usize = 65;
        let wasm_bytes = vec![0; SIZE];
        let byte_code = ByteCode::new(wasm_bytes);
        // String output is less than the bytes itself
        assert_eq!(
            format!("{:?}", byte_code),
            "ByteCode(0x00000000000000000000000000000000...)"
        );
    }

    #[test]
    fn contract_wasm_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash =
            HashAddr::try_from(&bytes[..]).expect("should create contract wasm hash");
        let contract_hash = ByteCodeHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_wasm_hash_from_str() {
        let contract_hash = ByteCodeHash([3; 32]);
        let encoded = contract_hash.to_formatted_string();
        let decoded = ByteCodeHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(contract_hash, decoded);

        let invalid_prefix =
            "contractwasm-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(ByteCodeHash::from_formatted_str(invalid_prefix).is_err());

        let short_addr =
            "contract-wasm-00000000000000000000000000000000000000000000000000000000000000";
        assert!(ByteCodeHash::from_formatted_str(short_addr).is_err());

        let long_addr =
            "contract-wasm-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(ByteCodeHash::from_formatted_str(long_addr).is_err());

        let invalid_hex =
            "contract-wasm-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(ByteCodeHash::from_formatted_str(invalid_hex).is_err());
    }

    #[test]
    fn contract_wasm_hash_serde_roundtrip() {
        let contract_hash = ByteCodeHash([255; 32]);
        let serialized = bincode::serialize(&contract_hash).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(contract_hash, deserialized)
    }

    #[test]
    fn contract_wasm_hash_json_roundtrip() {
        let contract_hash = ByteCodeHash([255; 32]);
        let json_string = serde_json::to_string_pretty(&contract_hash).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(contract_hash, decoded)
    }
}
