use alloc::{format, string::String, vec::Vec};
use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    addressable_entity, bytesrepr,
    bytesrepr::{Bytes, Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex, uref, CLType, CLTyped, HashAddr,
};

const BYTE_CODE_MAX_DISPLAY_LEN: usize = 16;
const KEY_HASH_LENGTH: usize = 32;
const WASM_STRING_PREFIX: &str = "byte-code-";

const BYTE_CODE_PREFIX: &str = "byte-code-";
const V1_WASM_PREFIX: &str = "v1-wasm-";
const EMPTY_PREFIX: &str = "empty-";

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

/// An address for ByteCode records stored in global state.
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum ByteCodeAddr {
    /// An address for byte code to be executed against the V1 Casper execution engine.
    V1CasperWasm(HashAddr),
    /// An empty byte code record
    Empty,
}

impl ByteCodeAddr {
    /// Constructs a new Byte code address for Wasm.
    pub const fn new_wasm_addr(hash_addr: HashAddr) -> Self {
        Self::V1CasperWasm(hash_addr)
    }

    /// Returns the tag of the byte code address.
    pub fn tag(&self) -> ByteCodeKind {
        match self {
            Self::Empty => ByteCodeKind::Empty,
            Self::V1CasperWasm(_) => ByteCodeKind::V1CasperWasm,
        }
    }

    /// Formats the `ByteCodeAddr` for users getting and putting.
    pub fn to_formatted_string(&self) -> String {
        format!("{}", self)
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a
    /// `ByteCodeAddr`.
    pub fn from_formatted_string(input: &str) -> Result<Self, FromStrError> {
        if let Some(byte_code) = input.strip_prefix(BYTE_CODE_PREFIX) {
            let (addr_str, tag) = if let Some(str) = byte_code.strip_prefix(EMPTY_PREFIX) {
                (str, ByteCodeKind::Empty)
            } else if let Some(str) = byte_code.strip_prefix(V1_WASM_PREFIX) {
                (str, ByteCodeKind::V1CasperWasm)
            } else {
                return Err(FromStrError::InvalidPrefix);
            };
            let addr = checksummed_hex::decode(addr_str).map_err(FromStrError::Hex)?;
            let byte_code_addr = HashAddr::try_from(addr.as_ref()).map_err(FromStrError::Hash)?;
            return match tag {
                ByteCodeKind::V1CasperWasm => Ok(ByteCodeAddr::V1CasperWasm(byte_code_addr)),
                ByteCodeKind::Empty => Ok(ByteCodeAddr::Empty),
            };
        }

        Err(FromStrError::InvalidPrefix)
    }
}

impl ToBytes for ByteCodeAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Self::Empty => 0,
                Self::V1CasperWasm(_) => KEY_HASH_LENGTH,
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            Self::Empty => writer.push(self.tag() as u8),
            Self::V1CasperWasm(addr) => {
                writer.push(self.tag() as u8);
                writer.extend(addr.to_bytes()?);
            }
        }
        Ok(())
    }
}

impl FromBytes for ByteCodeAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, remainder): (ByteCodeKind, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            ByteCodeKind::Empty => Ok((ByteCodeAddr::Empty, remainder)),
            ByteCodeKind::V1CasperWasm => {
                let (addr, remainder) = HashAddr::from_bytes(remainder)?;
                Ok((ByteCodeAddr::new_wasm_addr(addr), remainder))
            }
        }
    }
}

impl Display for ByteCodeAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ByteCodeAddr::V1CasperWasm(addr) => {
                write!(
                    f,
                    "{}{}{}",
                    BYTE_CODE_PREFIX,
                    V1_WASM_PREFIX,
                    base16::encode_lower(&addr)
                )
            }
            ByteCodeAddr::Empty => {
                write!(
                    f,
                    "{}{}{}",
                    BYTE_CODE_PREFIX,
                    EMPTY_PREFIX,
                    base16::encode_lower(&[0u8; 32])
                )
            }
        }
    }
}

impl Debug for ByteCodeAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ByteCodeAddr::V1CasperWasm(addr) => {
                write!(f, "ByteCodeAddr::V1CasperWasm({:?})", addr)
            }
            ByteCodeAddr::Empty => {
                write!(f, "ByteCodeAddr::Empty")
            }
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<ByteCodeAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ByteCodeAddr {
        match rng.gen_range(0..=1) {
            1 => ByteCodeAddr::V1CasperWasm(rng.gen()),
            0 => ByteCodeAddr::Empty,
            _ => unreachable!(),
        }
    }
}

/// A newtype wrapping a `HashAddr` which is the raw bytes of
/// the ByteCodeHash
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ByteCodeHash(HashAddr);

impl ByteCodeHash {
    /// Constructs a new `ByteCodeHash` from the raw bytes of the contract wasm hash.
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

    /// Formats the `ByteCodeHash` for users getting and putting.
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

impl Default for ByteCodeHash {
    fn default() -> Self {
        Self::new([0u8; KEY_HASH_LENGTH])
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
        self.0.write_bytes(writer)
    }
}

impl FromBytes for ByteCodeHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((ByteCodeHash::new(bytes), rem))
    }
}

impl From<[u8; KEY_HASH_LENGTH]> for ByteCodeHash {
    fn from(bytes: [u8; KEY_HASH_LENGTH]) -> Self {
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
        String::from("ByteCodeHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description =
            Some("The hash address of the contract wasm".to_string());
        schema_object.into()
    }
}

/// The type of Byte code.
#[repr(u8)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ByteCodeKind {
    /// Empty byte code.
    Empty = 0,
    /// Byte code to be executed with the version 1 Casper execution engine.
    V1CasperWasm = 1,
}

impl ToBytes for ByteCodeKind {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        (*self as u8).to_bytes()
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        (*self as u8).write_bytes(writer)
    }
}

impl FromBytes for ByteCodeKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (byte_code_kind, remainder) = u8::from_bytes(bytes)?;
        match byte_code_kind {
            byte_code_kind if byte_code_kind == ByteCodeKind::Empty as u8 => {
                Ok((ByteCodeKind::Empty, remainder))
            }
            byte_code_kind if byte_code_kind == ByteCodeKind::V1CasperWasm as u8 => {
                Ok((ByteCodeKind::V1CasperWasm, remainder))
            }
            _ => Err(Error::Formatting),
        }
    }
}

impl Display for ByteCodeKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ByteCodeKind::Empty => {
                write!(f, "empty")
            }
            ByteCodeKind::V1CasperWasm => {
                write!(f, "v1-casper-wasm")
            }
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<ByteCodeKind> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ByteCodeKind {
        match rng.gen_range(0..=1) {
            0 => ByteCodeKind::Empty,
            1 => ByteCodeKind::V1CasperWasm,
            _ => unreachable!(),
        }
    }
}

/// A container for contract's Wasm bytes.
#[derive(PartialEq, Eq, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct ByteCode {
    kind: ByteCodeKind,
    bytes: Bytes,
}

impl Debug for ByteCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.bytes.len() > BYTE_CODE_MAX_DISPLAY_LEN {
            write!(
                f,
                "ByteCode(0x{}...)",
                base16::encode_lower(&self.bytes[..BYTE_CODE_MAX_DISPLAY_LEN])
            )
        } else {
            write!(f, "ByteCode(0x{})", base16::encode_lower(&self.bytes))
        }
    }
}

impl ByteCode {
    /// Creates new Wasm object from bytes.
    pub fn new(kind: ByteCodeKind, bytes: Vec<u8>) -> Self {
        ByteCode {
            kind,
            bytes: bytes.into(),
        }
    }

    /// Consumes instance of [`ByteCode`] and returns its bytes.
    pub fn take_bytes(self) -> Vec<u8> {
        self.bytes.into()
    }

    /// Returns a slice of contained Wasm bytes.
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_ref()
    }

    /// Return the type of byte code.
    pub fn kind(&self) -> ByteCodeKind {
        self.kind
    }
}

impl ToBytes for ByteCode {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.kind.serialized_length() + self.bytes.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.kind.write_bytes(writer)?;
        self.bytes.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for ByteCode {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (kind, remainder) = ByteCodeKind::from_bytes(bytes)?;
        let (bytes, remainder) = Bytes::from_bytes(remainder)?;
        Ok((ByteCode { kind, bytes }, remainder))
    }
}

#[cfg(test)]
mod tests {
    use rand::RngCore;

    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn debug_repr_of_short_wasm() {
        const SIZE: usize = 8;
        let wasm_bytes = vec![0; SIZE];
        let byte_code = ByteCode::new(ByteCodeKind::V1CasperWasm, wasm_bytes);
        assert_eq!(format!("{:?}", byte_code), "ByteCode(0x0000000000000000)");
    }

    #[test]
    fn debug_repr_of_long_wasm() {
        const SIZE: usize = 65;
        let wasm_bytes = vec![0; SIZE];
        let byte_code = ByteCode::new(ByteCodeKind::V1CasperWasm, wasm_bytes);
        // String output is less than the bytes itself
        assert_eq!(
            format!("{:?}", byte_code),
            "ByteCode(0x00000000000000000000000000000000...)"
        );
    }

    #[test]
    fn byte_code_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let byte_code = ByteCode::new(rng.gen(), vec![]);
        bytesrepr::test_serialization_roundtrip(&byte_code);

        let mut buffer = vec![0u8; rng.gen_range(1..100)];
        rng.fill_bytes(buffer.as_mut());
        let byte_code = ByteCode::new(rng.gen(), buffer);
        bytesrepr::test_serialization_roundtrip(&byte_code);
    }

    #[test]
    fn contract_wasm_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let byte_code_hash = HashAddr::try_from(&bytes[..]).expect("should create byte code hash");
        let contract_hash = ByteCodeHash::new(byte_code_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_wasm_hash_from_str() {
        let byte_code_hash = ByteCodeHash([3; KEY_HASH_LENGTH]);
        let encoded = byte_code_hash.to_formatted_string();
        let decoded = ByteCodeHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(byte_code_hash, decoded);

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
    fn byte_code_addr_from_str() {
        let empty_addr = ByteCodeAddr::Empty;
        let encoded = empty_addr.to_formatted_string();
        let decoded = ByteCodeAddr::from_formatted_string(&encoded).unwrap();
        assert_eq!(empty_addr, decoded);

        let wasm_addr = ByteCodeAddr::V1CasperWasm([3; 32]);
        let encoded = wasm_addr.to_formatted_string();
        let decoded = ByteCodeAddr::from_formatted_string(&encoded).unwrap();
        assert_eq!(wasm_addr, decoded);
    }

    #[test]
    fn byte_code_serialization_roundtrip() {
        let rng = &mut TestRng::new();
        let wasm_addr = ByteCodeAddr::V1CasperWasm(rng.gen());
        bytesrepr::test_serialization_roundtrip(&wasm_addr);

        let empty_addr = ByteCodeAddr::Empty;
        bytesrepr::test_serialization_roundtrip(&empty_addr);
    }

    #[test]
    fn contract_wasm_hash_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let byte_code_hash = ByteCodeHash(rng.gen());
        bytesrepr::test_serialization_roundtrip(&byte_code_hash);
    }

    #[test]
    fn contract_wasm_hash_bincode_roundtrip() {
        let rng = &mut TestRng::new();
        let byte_code_hash = ByteCodeHash(rng.gen());
        let serialized = bincode::serialize(&byte_code_hash).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(byte_code_hash, deserialized)
    }

    #[test]
    fn contract_wasm_hash_json_roundtrip() {
        let rng = &mut TestRng::new();
        let byte_code_hash = ByteCodeHash(rng.gen());
        let json_string = serde_json::to_string_pretty(&byte_code_hash).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(byte_code_hash, decoded)
    }
}
