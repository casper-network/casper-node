// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;
use hex_fmt::HexFmt;
#[cfg(feature = "std")]
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    account::{self, AccountHash, TryFromSliceForAccountHashError},
    auction::EraId,
    bytesrepr::{self, Error, FromBytes, ToBytes, U64_SERIALIZED_LENGTH},
    uref::{self, URef, UREF_SERIALIZED_LENGTH},
    CLType, CLTyped, DeployHash, TransferAddr, DEPLOY_HASH_LENGTH, TRANSFER_ADDR_LENGTH,
};

const ACCOUNT_ID: u8 = 0;
const HASH_ID: u8 = 1;
const UREF_ID: u8 = 2;
const TRANSFER_ID: u8 = 3;
const DEPLOY_INFO_ID: u8 = 4;
const ERA_INFO_ID: u8 = 5;

const HASH_PREFIX: &str = "hash-";
const DEPLOY_INFO_PREFIX: &str = "deploy-";
const ERA_INFO_PREFIX: &str = "era-";
const CONTRACT_STRING_PREFIX: &str = "contract-";
const WASM_STRING_PREFIX: &str = "contract-wasm-";
const PACKAGE_STRING_PREFIX: &str = "contract-package-wasm";

/// The number of bytes in a Blake2b hash
pub const BLAKE2B_DIGEST_LENGTH: usize = 32;
/// The number of bytes in a [`Key::Hash`].
pub const KEY_HASH_LENGTH: usize = 32;
/// The number of bytes in a [`Key::Transfer`].
pub const KEY_TRANSFER_LENGTH: usize = TRANSFER_ADDR_LENGTH;
/// The number of bytes in a [`Key::DeployInfo`].
pub const KEY_DEPLOY_INFO_LENGTH: usize = DEPLOY_HASH_LENGTH;

const KEY_ID_SERIALIZED_LENGTH: usize = 1;
// u8 used to determine the ID
const KEY_HASH_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_HASH_LENGTH;
const KEY_UREF_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + UREF_SERIALIZED_LENGTH;
const KEY_TRANSFER_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_TRANSFER_LENGTH;
const KEY_DEPLOY_INFO_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_DEPLOY_INFO_LENGTH;
const KEY_ERA_INFO_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + U64_SERIALIZED_LENGTH;

/// An alias for [`Key`]s hash variant.
pub type HashAddr = [u8; KEY_HASH_LENGTH];

impl From<HashAddr> for Key {
    fn from(addr: HashAddr) -> Self {
        Key::Hash(addr)
    }
}

/// An alias for [`Key`]s hash variant.
pub type ContractHashBytes = HashAddr;

/// Associated error type of `TryFrom<&[u8]>` for [`ContractHash`].
#[derive(Debug)]
pub struct TryFromSliceForContractHashError(());

impl Display for TryFromSliceForContractHashError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "failed to retrieve from slice")
    }
}

/// A newtype wrapping a [`ContractHashBytes`] which is the raw bytes of
/// the ContractHash
#[derive(DataSize, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ContractHash(ContractHashBytes);

impl ContractHash {
    /// Constructs a new `ContractHash` from the raw bytes of the contract hash.
    pub const fn new(value: ContractHashBytes) -> ContractHash {
        ContractHash(value)
    }
    /// Returns the raw bytes of the contract hash as an array.
    pub fn value(&self) -> ContractHashBytes {
        self.0
    }
    /// Returns the raw bytes of the contract hash as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Formats the `ContractHash` for users getting and putting.
    pub fn to_formatted_string(&self) -> String {
        format!(
            "{}{}",
            CONTRACT_STRING_PREFIX,
            base16::encode_lower(&self.0),
        )
    }
    /// Parses Parses a string formatted as per `Self::to_formatted_string()` into an
    /// `ContractHash`.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(CONTRACT_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = ContractHashBytes::try_from(base16::decode(remainder)?.as_ref())?;
        Ok(ContractHash(bytes))
    }
}

impl Display for ContractHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for ContractHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "ContractHash({})", base16::encode_lower(&self.0))
    }
}

impl CLTyped for ContractHash {
    fn cl_type() -> CLType {
        CLType::ByteArray(KEY_HASH_LENGTH as u32)
    }
}

impl ToBytes for ContractHash {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for ContractHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((ContractHash::new(bytes), rem))
    }
}

impl From<[u8; 32]> for ContractHash {
    fn from(bytes: [u8; 32]) -> Self {
        ContractHash(bytes)
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
            ContractHash::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = ContractHashBytes::deserialize(deserializer)?;
            Ok(ContractHash(bytes))
        }
    }
}

impl AsRef<[u8]> for ContractHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TryFrom<&[u8]> for ContractHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForContractHashError> {
        ContractHashBytes::try_from(bytes)
            .map(ContractHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

impl TryFrom<&alloc::vec::Vec<u8>> for ContractHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        ContractHashBytes::try_from(bytes as &[u8])
            .map(ContractHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

#[cfg(feature = "std")]
impl JsonSchema for ContractHash {
    fn schema_name() -> String {
        String::from("ContractHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some("The hash address of the contract".to_string());
        schema_object.into()
    }
}

/// An alias for [`Key`]s hash variant.
pub type ContractWasmHashBytes = HashAddr;

/// A newtype wrapping a [`ContractHashBytes`] which is the raw bytes of
/// the ContractWasmHash
#[derive(DataSize, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ContractWasmHash(ContractWasmHashBytes);

impl ContractWasmHash {
    /// Constructs a new `ContractHash` from the raw bytes of the contract hash.
    pub const fn new(value: ContractWasmHashBytes) -> ContractWasmHash {
        ContractWasmHash(value)
    }
    /// Returns the raw bytes of the contract hash as an array.
    pub fn value(&self) -> ContractWasmHashBytes {
        self.0
    }
    /// Returns the raw bytes of the contract hash as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Formats the `ContractHash` for users getting and putting.
    pub fn to_formatted_string(&self) -> String {
        format!("{}{}", WASM_STRING_PREFIX, base16::encode_lower(&self.0),)
    }
    /// Parses Parses a string formatted as per `Self::to_formatted_string()` into an
    /// `ContractWasmHash`.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(WASM_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = ContractHashBytes::try_from(base16::decode(remainder)?.as_ref())?;
        Ok(ContractWasmHash(bytes))
    }
}

impl Display for ContractWasmHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for ContractWasmHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "ContractWasmHash({})", base16::encode_lower(&self.0))
    }
}
impl CLTyped for ContractWasmHash {
    fn cl_type() -> CLType {
        CLType::ByteArray(KEY_HASH_LENGTH as u32)
    }
}

impl ToBytes for ContractWasmHash {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for ContractWasmHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((ContractWasmHash::new(bytes), rem))
    }
}

impl From<[u8; 32]> for ContractWasmHash {
    fn from(bytes: [u8; 32]) -> Self {
        ContractWasmHash(bytes)
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
            ContractWasmHash::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = ContractWasmHashBytes::deserialize(deserializer)?;
            Ok(ContractWasmHash(bytes))
        }
    }
}

impl AsRef<[u8]> for ContractWasmHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TryFrom<&[u8]> for ContractWasmHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForContractHashError> {
        ContractWasmHashBytes::try_from(bytes)
            .map(ContractWasmHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

impl TryFrom<&alloc::vec::Vec<u8>> for ContractWasmHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        ContractWasmHashBytes::try_from(bytes as &[u8])
            .map(ContractWasmHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

#[cfg(feature = "std")]
impl JsonSchema for ContractWasmHash {
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

/// An alias for [`Key`]s hash variant.
pub type ContractPackageHashBytes = HashAddr;

/// A newtype wrapping a [`ContractHashBytes`] which is the raw bytes of
/// the ContractPackageHash
#[derive(DataSize, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ContractPackageHash(ContractPackageHashBytes);

impl ContractPackageHash {
    /// Constructs a new `ContractHash` from the raw bytes of the contract hash.
    pub const fn new(value: ContractPackageHashBytes) -> ContractPackageHash {
        ContractPackageHash(value)
    }
    /// Returns the raw bytes of the contract hash as an array.
    pub fn value(&self) -> ContractPackageHashBytes {
        self.0
    }
    /// Returns the raw bytes of the contract hash as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Formats the `ContractHash` for users getting and putting.
    pub fn to_formatted_string(&self) -> String {
        format!("{}{}", PACKAGE_STRING_PREFIX, base16::encode_lower(&self.0),)
    }
    /// Parses Parses a string formatted as per `Self::to_formatted_string()` into an
    /// `ContractPackageHash`.
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(PACKAGE_STRING_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes = ContractHashBytes::try_from(base16::decode(remainder)?.as_ref())?;
        Ok(ContractPackageHash(bytes))
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
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for ContractPackageHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
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
            let bytes = ContractWasmHashBytes::deserialize(deserializer)?;
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
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForContractHashError> {
        ContractPackageHashBytes::try_from(bytes)
            .map(ContractPackageHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

impl TryFrom<&alloc::vec::Vec<u8>> for ContractPackageHash {
    type Error = TryFromSliceForContractHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        ContractPackageHashBytes::try_from(bytes as &[u8])
            .map(ContractPackageHash::new)
            .map_err(|_| TryFromSliceForContractHashError(()))
    }
}

#[cfg(feature = "std")]
impl JsonSchema for ContractPackageHash {
    fn schema_name() -> String {
        String::from("ContractPackageHash")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description =
            Some("The hash address of the contract package".to_string());
        schema_object.into()
    }
}

/// The type under which data (e.g. [`CLValue`](crate::CLValue)s, smart contracts, user accounts)
/// are indexed on the network.
#[repr(C)]
#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash)]
pub enum Key {
    /// A `Key` under which a user account is stored.
    Account(AccountHash),
    /// A `Key` under which a smart contract is stored and which is the pseudo-hash of the
    /// contract.
    Hash(HashAddr),
    /// A `Key` which is a [`URef`], under which most types of data can be stored.
    URef(URef),
    /// A `Key` under which we store a transfer.
    Transfer(TransferAddr),
    /// A `Key` under which we store a deploy info.
    DeployInfo(DeployHash),
    /// A `Key` under which we store an era info.
    EraInfo(EraId),
}

#[derive(Debug)]
pub enum FromStrError {
    InvalidPrefix,
    Hex(base16::DecodeError),
    Account(TryFromSliceForAccountHashError),
    Hash(TryFromSliceError),
    AccountHash(account::FromStrError),
    URef(uref::FromStrError),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

impl From<TryFromSliceForAccountHashError> for FromStrError {
    fn from(error: TryFromSliceForAccountHashError) -> Self {
        FromStrError::Account(error)
    }
}

impl From<TryFromSliceError> for FromStrError {
    fn from(error: TryFromSliceError) -> Self {
        FromStrError::Hash(error)
    }
}

impl From<account::FromStrError> for FromStrError {
    fn from(error: account::FromStrError) -> Self {
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
            FromStrError::Account(error) => write!(f, "account from string error: {:?}", error),
            FromStrError::Hash(error) => write!(f, "hash from string error: {}", error),
            FromStrError::AccountHash(error) => {
                write!(f, "account hash from string error: {:?}", error)
            }
            FromStrError::URef(error) => write!(f, "uref from string error: {:?}", error),
        }
    }
}

impl Key {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn type_string(&self) -> String {
        match self {
            Key::Account(_) => String::from("Key::Account"),
            Key::Hash(_) => String::from("Key::Hash"),
            Key::URef(_) => String::from("Key::URef"),
            Key::Transfer(_) => String::from("Key::Transfer"),
            Key::DeployInfo(_) => String::from("Key::DeployInfo"),
            Key::EraInfo(_) => String::from("Key::EraInfo"),
        }
    }

    /// Returns the maximum size a [`Key`] can be serialized into.
    pub const fn max_serialized_length() -> usize {
        KEY_UREF_SERIALIZED_LENGTH
    }

    /// If `self` is of type [`Key::URef`], returns `self` with the
    /// [`AccessRights`](crate::AccessRights) stripped from the wrapped [`URef`], otherwise
    /// returns `self` unmodified.
    pub fn normalize(self) -> Key {
        match self {
            Key::URef(uref) => Key::URef(uref.remove_access_rights()),
            other => other,
        }
    }

    /// Returns a human-readable version of `self`, with the inner bytes encoded to Base16.
    pub fn to_formatted_string(&self) -> String {
        match self {
            Key::Account(account_hash) => account_hash.to_formatted_string(),
            Key::Hash(addr) => format!("{}{}", HASH_PREFIX, base16::encode_lower(addr)),
            Key::URef(uref) => uref.to_formatted_string(),
            Key::Transfer(transfer_addr) => transfer_addr.to_formatted_string(),
            Key::DeployInfo(addr) => {
                format!(
                    "{}{}",
                    DEPLOY_INFO_PREFIX,
                    base16::encode_lower(addr.as_bytes())
                )
            }
            Key::EraInfo(era_id) => {
                format!("{}{}", ERA_INFO_PREFIX, era_id.to_string())
            }
        }
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a `Key`.
    pub fn from_formatted_str(input: &str) -> Result<Key, FromStrError> {
        if let Ok(account_hash) = AccountHash::from_formatted_str(input) {
            Ok(Key::Account(account_hash))
        } else if let Some(hex) = input.strip_prefix(HASH_PREFIX) {
            Ok(Key::Hash(HashAddr::try_from(
                base16::decode(hex)?.as_ref(),
            )?))
        } else if let Some(hex) = input.strip_prefix(DEPLOY_INFO_PREFIX) {
            Ok(Key::DeployInfo(DeployHash::new(
                <[u8; DEPLOY_HASH_LENGTH]>::try_from(base16::decode(hex)?.as_ref())?,
            )))
        } else if let Ok(transfer_addr) = TransferAddr::from_formatted_str(input) {
            Ok(Key::Transfer(transfer_addr))
        } else {
            Ok(Key::URef(URef::from_formatted_str(input)?))
        }
    }

    /// Returns the inner bytes of `self` if `self` is of type [`Key::Account`], otherwise returns
    /// `None`.
    pub fn into_account(self) -> Option<AccountHash> {
        match self {
            Key::Account(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Returns the inner bytes of `self` if `self` is of type [`Key::Hash`], otherwise returns
    /// `None`.
    pub fn into_hash(self) -> Option<HashAddr> {
        match self {
            Key::Hash(hash) => Some(hash),
            _ => None,
        }
    }

    /// Returns a reference to the inner [`URef`] if `self` is of type [`Key::URef`], otherwise
    /// returns `None`.
    pub fn as_uref(&self) -> Option<&URef> {
        match self {
            Key::URef(uref) => Some(uref),
            _ => None,
        }
    }

    /// Returns the inner [`URef`] if `self` is of type [`Key::URef`], otherwise returns `None`.
    pub fn into_uref(self) -> Option<URef> {
        match self {
            Key::URef(uref) => Some(uref),
            _ => None,
        }
    }

    // TODO: remove this nightmare
    /// Creates the seed of a local key for a context with the given base key.
    pub fn into_seed(self) -> [u8; BLAKE2B_DIGEST_LENGTH] {
        match self {
            Key::Account(account_hash) => account_hash.value(),
            Key::Hash(bytes) => bytes,
            Key::URef(uref) => uref.addr(),
            Key::Transfer(transfer_addr) => transfer_addr.value(),
            Key::DeployInfo(addr) => addr.value(),
            Key::EraInfo(era_id) => {
                // stop-gap measure until this method is removed
                let mut ret = [0u8; BLAKE2B_DIGEST_LENGTH];
                let era_id_bytes = era_id.to_le_bytes();
                let era_id_bytes_len = era_id_bytes.len();
                assert!(era_id_bytes_len < BLAKE2B_DIGEST_LENGTH);
                ret[..era_id_bytes_len].clone_from_slice(&era_id_bytes);
                ret
            }
        }
    }

    /// Casts a [`Key::URef`] to a [`Key::Hash`]
    pub fn uref_to_hash(&self) -> Option<Key> {
        let uref = self.as_uref()?;
        let addr = uref.addr();
        Some(Key::Hash(addr))
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Key::Account(account_hash) => write!(f, "Key::Account({})", account_hash),
            Key::Hash(addr) => write!(f, "Key::Hash({})", HexFmt(addr)),
            Key::URef(uref) => write!(f, "Key::{}", uref), /* Display impl for URef will append */
            Key::Transfer(transfer_addr) => write!(f, "Key::Transfer({})", transfer_addr),
            Key::DeployInfo(addr) => write!(f, "Key::DeployInfo({})", HexFmt(addr.as_bytes())),
            Key::EraInfo(era_id) => write!(f, "Key::EraInfo({})", era_id),
        }
    }
}

impl Debug for Key {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl From<URef> for Key {
    fn from(uref: URef) -> Key {
        Key::URef(uref)
    }
}

impl From<AccountHash> for Key {
    fn from(account_hash: AccountHash) -> Key {
        Key::Account(account_hash)
    }
}

impl From<TransferAddr> for Key {
    fn from(transfer_addr: TransferAddr) -> Key {
        Key::Transfer(transfer_addr)
    }
}

impl From<ContractHash> for Key {
    fn from(contract_hash: ContractHash) -> Key {
        Key::Hash(contract_hash.value())
    }
}

impl From<ContractWasmHash> for Key {
    fn from(wasm_hash: ContractWasmHash) -> Key {
        Key::Hash(wasm_hash.value())
    }
}

impl From<ContractPackageHash> for Key {
    fn from(package_hash: ContractPackageHash) -> Key {
        Key::Hash(package_hash.value())
    }
}

impl ToBytes for Key {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        match self {
            Key::Account(account_hash) => {
                result.push(ACCOUNT_ID);
                result.append(&mut account_hash.to_bytes()?);
            }
            Key::Hash(hash) => {
                result.push(HASH_ID);
                result.append(&mut hash.to_bytes()?);
            }
            Key::URef(uref) => {
                result.push(UREF_ID);
                result.append(&mut uref.to_bytes()?);
            }
            Key::Transfer(addr) => {
                result.push(TRANSFER_ID);
                result.append(&mut addr.to_bytes()?);
            }
            Key::DeployInfo(addr) => {
                result.push(DEPLOY_INFO_ID);
                result.append(&mut addr.to_bytes()?);
            }
            Key::EraInfo(era_id) => {
                result.push(ERA_INFO_ID);
                result.append(&mut era_id.to_bytes()?);
            }
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        match self {
            Key::Account(account_hash) => {
                KEY_ID_SERIALIZED_LENGTH + account_hash.serialized_length()
            }
            Key::Hash(_) => KEY_HASH_SERIALIZED_LENGTH,
            Key::URef(_) => KEY_UREF_SERIALIZED_LENGTH,
            Key::Transfer(_) => KEY_TRANSFER_SERIALIZED_LENGTH,
            Key::DeployInfo(_) => KEY_DEPLOY_INFO_SERIALIZED_LENGTH,
            Key::EraInfo(_) => KEY_ERA_INFO_SERIALIZED_LENGTH,
        }
    }
}

impl FromBytes for Key {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (id, remainder) = u8::from_bytes(bytes)?;
        match id {
            ACCOUNT_ID => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Account(account_hash), rem))
            }
            HASH_ID => {
                let (hash, rem) = FromBytes::from_bytes(remainder)?;
                Ok((Key::Hash(hash), rem))
            }
            UREF_ID => {
                let (uref, rem) = URef::from_bytes(remainder)?;
                Ok((Key::URef(uref), rem))
            }
            TRANSFER_ID => {
                let (transfer_addr, rem) = TransferAddr::from_bytes(remainder)?;
                Ok((Key::Transfer(transfer_addr), rem))
            }
            DEPLOY_INFO_ID => {
                let (deploy_hash, rem) = FromBytes::from_bytes(remainder)?;
                Ok((Key::DeployInfo(deploy_hash), rem))
            }
            ERA_INFO_ID => {
                let (era_id, rem) = FromBytes::from_bytes(remainder)?;
                Ok((Key::EraInfo(era_id), rem))
            }
            _ => Err(Error::Formatting),
        }
    }
}

mod serde_helpers {
    use super::*;

    #[derive(Serialize, Deserialize)]
    pub(super) enum HumanReadable {
        Account(String),
        Hash(String),
        URef(String),
        Transfer(String),
        DeployInfo(String),
        EraInfo(String),
    }

    impl From<&Key> for HumanReadable {
        fn from(key: &Key) -> Self {
            let formatted_string = key.to_formatted_string();
            match key {
                Key::Account(_) => HumanReadable::Account(formatted_string),
                Key::Hash(_) => HumanReadable::Hash(formatted_string),
                Key::URef(_) => HumanReadable::URef(formatted_string),
                Key::Transfer(_) => HumanReadable::Transfer(formatted_string),
                Key::DeployInfo(_) => HumanReadable::DeployInfo(formatted_string),
                Key::EraInfo(_) => HumanReadable::EraInfo(formatted_string),
            }
        }
    }

    impl TryFrom<HumanReadable> for Key {
        type Error = FromStrError;

        fn try_from(helper: HumanReadable) -> Result<Self, Self::Error> {
            match helper {
                HumanReadable::Account(formatted_string)
                | HumanReadable::Hash(formatted_string)
                | HumanReadable::URef(formatted_string)
                | HumanReadable::Transfer(formatted_string)
                | HumanReadable::DeployInfo(formatted_string)
                | HumanReadable::EraInfo(formatted_string) => {
                    Key::from_formatted_str(&formatted_string)
                }
            }
        }
    }

    #[derive(Serialize)]
    pub(super) enum BinarySerHelper<'a> {
        Account(&'a AccountHash),
        Hash(&'a HashAddr),
        URef(&'a URef),
        Transfer(&'a TransferAddr),
        DeployInfo(&'a DeployHash),
        EraInfo(&'a u64),
    }

    impl<'a> From<&'a Key> for BinarySerHelper<'a> {
        fn from(key: &'a Key) -> Self {
            match key {
                Key::Account(account_hash) => BinarySerHelper::Account(account_hash),
                Key::Hash(hash_addr) => BinarySerHelper::Hash(hash_addr),
                Key::URef(uref) => BinarySerHelper::URef(uref),
                Key::Transfer(transfer_addr) => BinarySerHelper::Transfer(transfer_addr),
                Key::DeployInfo(deploy_hash) => BinarySerHelper::DeployInfo(deploy_hash),
                Key::EraInfo(era_id) => BinarySerHelper::EraInfo(era_id),
            }
        }
    }

    #[derive(Deserialize)]
    pub(super) enum BinaryDeserHelper {
        Account(AccountHash),
        Hash(HashAddr),
        URef(URef),
        Transfer(TransferAddr),
        DeployInfo(DeployHash),
        EraInfo(EraId),
    }

    impl From<BinaryDeserHelper> for Key {
        fn from(helper: BinaryDeserHelper) -> Self {
            match helper {
                BinaryDeserHelper::Account(account_hash) => Key::Account(account_hash),
                BinaryDeserHelper::Hash(hash_addr) => Key::Hash(hash_addr),
                BinaryDeserHelper::URef(uref) => Key::URef(uref),
                BinaryDeserHelper::Transfer(transfer_addr) => Key::Transfer(transfer_addr),
                BinaryDeserHelper::DeployInfo(deploy_hash) => Key::DeployInfo(deploy_hash),
                BinaryDeserHelper::EraInfo(era_id) => Key::EraInfo(era_id),
            }
        }
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serde_helpers::HumanReadable::from(self).serialize(serializer)
        } else {
            serde_helpers::BinarySerHelper::from(self).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let human_readable = serde_helpers::HumanReadable::deserialize(deserializer)?;
            Key::try_from(human_readable).map_err(SerdeError::custom)
        } else {
            let binary_helper = serde_helpers::BinaryDeserHelper::deserialize(deserializer)?;
            Ok(Key::from(binary_helper))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bytesrepr::{Error, FromBytes},
        AccessRights, URef,
    };

    fn test_readable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_readable(), is_true)
    }

    #[test]
    fn test_is_readable() {
        test_readable(AccessRights::READ, true);
        test_readable(AccessRights::READ_ADD, true);
        test_readable(AccessRights::READ_WRITE, true);
        test_readable(AccessRights::READ_ADD_WRITE, true);
        test_readable(AccessRights::ADD, false);
        test_readable(AccessRights::ADD_WRITE, false);
        test_readable(AccessRights::WRITE, false);
    }

    fn test_writable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_writeable(), is_true)
    }

    #[test]
    fn test_is_writable() {
        test_writable(AccessRights::WRITE, true);
        test_writable(AccessRights::READ_WRITE, true);
        test_writable(AccessRights::ADD_WRITE, true);
        test_writable(AccessRights::READ, false);
        test_writable(AccessRights::ADD, false);
        test_writable(AccessRights::READ_ADD, false);
        test_writable(AccessRights::READ_ADD_WRITE, true);
    }

    fn test_addable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_addable(), is_true)
    }

    #[test]
    fn test_is_addable() {
        test_addable(AccessRights::ADD, true);
        test_addable(AccessRights::READ_ADD, true);
        test_addable(AccessRights::READ_WRITE, false);
        test_addable(AccessRights::ADD_WRITE, true);
        test_addable(AccessRights::READ, false);
        test_addable(AccessRights::WRITE, false);
        test_addable(AccessRights::READ_ADD_WRITE, true);
    }

    #[test]
    fn should_display_key() {
        let expected_hash = core::iter::repeat("0").take(64).collect::<String>();
        let addr_array = [0u8; 32];
        let account_hash = AccountHash::new(addr_array);
        let account_key = Key::Account(account_hash);
        assert_eq!(
            format!("{}", account_key),
            format!("Key::Account({})", expected_hash)
        );
        let uref_key = Key::URef(URef::new(addr_array, AccessRights::READ));
        assert_eq!(
            format!("{}", uref_key),
            format!("Key::URef({}, READ)", expected_hash)
        );
        let hash_key = Key::Hash(addr_array);
        assert_eq!(
            format!("{}", hash_key),
            format!("Key::Hash({})", expected_hash)
        );
        let transfer_key = Key::Transfer(TransferAddr::new(addr_array));
        assert_eq!(
            format!("{}", transfer_key),
            format!("Key::Transfer({})", expected_hash)
        );
        let deploy_info_key = Key::DeployInfo(DeployHash::new(addr_array));
        assert_eq!(
            format!("{}", deploy_info_key),
            format!("Key::DeployInfo({})", expected_hash)
        );
    }

    #[test]
    fn abuse_vec_key() {
        // Prefix is 2^32-1 = shouldn't allocate that much
        let bytes: Vec<u8> = vec![255, 255, 255, 255, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let res: Result<(Vec<Key>, &[u8]), _> = FromBytes::from_bytes(&bytes);
        #[cfg(target_os = "linux")]
        assert_eq!(res.expect_err("should fail"), Error::OutOfMemory);
        #[cfg(target_os = "macos")]
        assert_eq!(res.expect_err("should fail"), Error::EarlyEndOfStream);
    }

    #[test]
    fn check_key_account_getters() {
        let account = [42; 32];
        let account_hash = AccountHash::new(account);
        let key1 = Key::Account(account_hash);
        assert_eq!(key1.into_account(), Some(account_hash));
        assert!(key1.into_hash().is_none());
        assert!(key1.as_uref().is_none());
    }

    #[test]
    fn check_key_hash_getters() {
        let hash = [42; KEY_HASH_LENGTH];
        let key1 = Key::Hash(hash);
        assert!(key1.into_account().is_none());
        assert_eq!(key1.into_hash(), Some(hash));
        assert!(key1.as_uref().is_none());
    }

    #[test]
    fn check_key_uref_getters() {
        let uref = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
        let key1 = Key::URef(uref);
        assert!(key1.into_account().is_none());
        assert!(key1.into_hash().is_none());
        assert_eq!(key1.as_uref(), Some(&uref));
    }

    #[test]
    fn key_max_serialized_length() {
        let key_account = Key::Account(AccountHash::new([42; BLAKE2B_DIGEST_LENGTH]));
        assert!(key_account.serialized_length() <= Key::max_serialized_length());

        let key_hash = Key::Hash([42; KEY_HASH_LENGTH]);
        assert!(key_hash.serialized_length() <= Key::max_serialized_length());

        let key_uref = Key::URef(URef::new([42; BLAKE2B_DIGEST_LENGTH], AccessRights::READ));
        assert!(key_uref.serialized_length() <= Key::max_serialized_length());

        let key_transfer = Key::Transfer(TransferAddr::new([42; BLAKE2B_DIGEST_LENGTH]));
        assert!(key_transfer.serialized_length() <= Key::max_serialized_length());

        let key_deploy_info = Key::DeployInfo(DeployHash::new([42; BLAKE2B_DIGEST_LENGTH]));
        assert!(key_deploy_info.serialized_length() <= Key::max_serialized_length());
    }

    fn round_trip(key: Key) {
        let string = key.to_formatted_string();
        let parsed_key = Key::from_formatted_str(&string).unwrap();
        assert_eq!(key, parsed_key);
    }

    #[test]
    fn key_from_str() {
        round_trip(Key::Account(AccountHash::new([42; BLAKE2B_DIGEST_LENGTH])));
        round_trip(Key::Hash([42; KEY_HASH_LENGTH]));
        round_trip(Key::URef(URef::new(
            [255; BLAKE2B_DIGEST_LENGTH],
            AccessRights::READ,
        )));
        round_trip(Key::Transfer(TransferAddr::new([42; KEY_HASH_LENGTH])));
        round_trip(Key::DeployInfo(DeployHash::new([42; KEY_HASH_LENGTH])));

        let invalid_prefix = "a-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(Key::from_formatted_str(invalid_prefix).is_err());

        let invalid_prefix = "hash0000000000000000000000000000000000000000000000000000000000000000";
        assert!(Key::from_formatted_str(invalid_prefix).is_err());

        let short_addr = "00000000000000000000000000000000000000000000000000000000000000";
        assert!(Key::from_formatted_str(&format!("{}{}", HASH_PREFIX, short_addr)).is_err());

        let long_addr = "000000000000000000000000000000000000000000000000000000000000000000";
        assert!(Key::from_formatted_str(&format!("{}{}", HASH_PREFIX, long_addr)).is_err());

        let invalid_hex = "000000000000000000000000000000000000000000000000000000000000000g";
        assert!(Key::from_formatted_str(&format!("{}{}", HASH_PREFIX, invalid_hex)).is_err());
    }

    #[test]
    fn key_to_json() {
        let array = [42; BLAKE2B_DIGEST_LENGTH];
        let hex_bytes = "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a";

        let key_account = Key::Account(AccountHash::new(array));
        assert_eq!(
            serde_json::to_string(&key_account).unwrap(),
            format!(r#"{{"Account":"account-hash-{}"}}"#, hex_bytes)
        );

        let key_hash = Key::Hash(array);
        assert_eq!(
            serde_json::to_string(&key_hash).unwrap(),
            format!(r#"{{"Hash":"hash-{}"}}"#, hex_bytes)
        );

        let key_uref = Key::URef(URef::new(array, AccessRights::READ));
        assert_eq!(
            serde_json::to_string(&key_uref).unwrap(),
            format!(r#"{{"URef":"uref-{}-001"}}"#, hex_bytes)
        );

        let key_transfer = Key::Transfer(TransferAddr::new(array));
        assert_eq!(
            serde_json::to_string(&key_transfer).unwrap(),
            format!(r#"{{"Transfer":"transfer-{}"}}"#, hex_bytes)
        );

        let key_deploy_info = Key::DeployInfo(DeployHash::new(array));
        assert_eq!(
            serde_json::to_string(&key_deploy_info).unwrap(),
            format!(r#"{{"DeployInfo":"deploy-{}"}}"#, hex_bytes)
        );
    }

    #[test]
    fn contract_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash = HashAddr::try_from(&bytes[..]).expect("should create contract hash");
        let contract_hash = ContractHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_wasm_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash =
            HashAddr::try_from(&bytes[..]).expect("should create contract wasm hash");
        let contract_hash = ContractWasmHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_package_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let contract_hash = HashAddr::try_from(&bytes[..]).expect("should create contract hash");
        let contract_hash = ContractPackageHash::new(contract_hash);
        assert_eq!(&bytes, &contract_hash.as_bytes());
    }

    #[test]
    fn contract_hash_from_str() {
        let contract_hash = ContractHash([3; 32]);
        let encoded = contract_hash.to_formatted_string();
        let decoded = ContractHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(contract_hash, decoded);

        let invalid_prefix =
            "contract--0000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_str(invalid_prefix).is_err());

        let short_addr = "contract-00000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_str(short_addr).is_err());

        let long_addr =
            "contract-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractHash::from_formatted_str(long_addr).is_err());

        let invalid_hex =
            "contract-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(ContractHash::from_formatted_str(invalid_hex).is_err());
    }

    #[test]
    fn contract_wasm_hash_from_str() {
        let contract_hash = ContractWasmHash([3; 32]);
        let encoded = contract_hash.to_formatted_string();
        let decoded = ContractWasmHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(contract_hash, decoded);

        let invalid_prefix =
            "contractwasm-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractWasmHash::from_formatted_str(invalid_prefix).is_err());

        let short_addr =
            "contract-wasm-00000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractWasmHash::from_formatted_str(short_addr).is_err());

        let long_addr =
            "contract-wasm-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractWasmHash::from_formatted_str(long_addr).is_err());

        let invalid_hex =
            "contract-wasm-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(ContractWasmHash::from_formatted_str(invalid_hex).is_err());
    }

    #[test]
    fn contract_package_hash_from_str() {
        let contract_hash = ContractPackageHash([3; 32]);
        let encoded = contract_hash.to_formatted_string();
        let decoded = ContractPackageHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(contract_hash, decoded);

        let invalid_prefix =
            "contractpackage-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractPackageHash::from_formatted_str(invalid_prefix).is_err());

        let short_addr =
            "contract-package-00000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractPackageHash::from_formatted_str(short_addr).is_err());

        let long_addr =
            "contract-package-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(ContractPackageHash::from_formatted_str(long_addr).is_err());

        let invalid_hex =
            "contract-package-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(ContractPackageHash::from_formatted_str(invalid_hex).is_err());
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
