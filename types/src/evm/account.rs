#[cfg(feature = "json-schema")]
use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Address, Hash, ADDRESS_LENGTH};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest, URef, U256,
};

/// Keccak-256 hash of empty EVM bytecode.
pub const EMPTY_CODE_HASH: Hash = Hash::new([
    0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7, 0x03, 0xc0,
    0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04, 0x5d, 0x85, 0xa4, 0x70,
]);

/// EVM account metadata stored in global state.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Account {
    nonce: u64,
    code_hash: Hash,
    main_purse: URef,
}

impl Account {
    /// Creates EVM account metadata.
    pub const fn new(nonce: u64, code_hash: Hash, main_purse: URef) -> Self {
        Account {
            nonce,
            code_hash,
            main_purse,
        }
    }

    /// Returns the EVM account nonce.
    pub const fn nonce(self) -> u64 {
        self.nonce
    }

    /// Returns the hash of the bytecode associated with this account.
    pub const fn code_hash(self) -> Hash {
        self.code_hash
    }

    /// Returns the Casper main purse backing this EVM account balance.
    pub const fn main_purse(self) -> URef {
        self.main_purse
    }
}

impl ToBytes for Account {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut bytes = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut bytes)?;
        Ok(bytes)
    }

    fn serialized_length(&self) -> usize {
        self.nonce.serialized_length()
            + self.code_hash.serialized_length()
            + self.main_purse.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.nonce.write_bytes(writer)?;
        self.code_hash.write_bytes(writer)?;
        self.main_purse.write_bytes(writer)
    }
}

impl FromBytes for Account {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (nonce, remainder) = u64::from_bytes(bytes)?;
        let (code_hash, remainder) = Hash::from_bytes(remainder)?;
        let (main_purse, remainder) = URef::from_bytes(remainder)?;
        Ok((Account::new(nonce, code_hash, main_purse), remainder))
    }
}

/// EVM storage value stored in global state.
#[derive(
    Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct StorageValue(#[cfg_attr(feature = "json-schema", schemars(with = "String"))] U256);

impl StorageValue {
    /// Creates an EVM storage value from a 256-bit word.
    pub const fn new(value: U256) -> Self {
        StorageValue(value)
    }

    /// Returns the 256-bit word stored in this slot.
    pub const fn value(self) -> U256 {
        self.0
    }

    /// Returns `true` when all bytes are zero.
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

impl From<U256> for StorageValue {
    fn from(value: U256) -> Self {
        StorageValue::new(value)
    }
}

impl From<StorageValue> for U256 {
    fn from(value: StorageValue) -> Self {
        value.value()
    }
}

impl ToBytes for StorageValue {
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

impl FromBytes for StorageValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        U256::from_bytes(bytes).map(|(value, remainder)| (StorageValue::new(value), remainder))
    }
}

/// Global-state address for one EVM account storage slot.
#[derive(
    Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StorageAddr {
    address: Address,
    #[cfg_attr(feature = "json-schema", schemars(with = "String"))]
    slot: U256,
}

impl StorageAddr {
    /// Creates an EVM storage address from a contract address and storage slot.
    pub const fn new(address: Address, slot: U256) -> Self {
        StorageAddr { address, slot }
    }

    /// Returns the EVM account or contract address owning the storage slot.
    pub const fn address(self) -> Address {
        self.address
    }

    /// Returns the EVM storage slot key.
    pub const fn slot(self) -> U256 {
        self.slot
    }
}

impl ToBytes for StorageAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut bytes = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut bytes)?;
        Ok(bytes)
    }

    fn serialized_length(&self) -> usize {
        self.address.serialized_length() + self.slot.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.address.write_bytes(writer)?;
        self.slot.write_bytes(writer)
    }
}

impl FromBytes for StorageAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (address, remainder) = Address::from_bytes(bytes)?;
        let (slot, remainder) = U256::from_bytes(remainder)?;
        Ok((StorageAddr::new(address, slot), remainder))
    }
}

/// Returns the deterministic main purse backing an EVM address.
pub fn deterministic_purse(address: Address) -> URef {
    let mut preimage = Vec::with_capacity(b"evm-purse-v1".len() + ADDRESS_LENGTH);
    preimage.extend_from_slice(b"evm-purse-v1");
    preimage.extend_from_slice(address.as_ref());
    URef::new(
        Digest::hash(preimage).value(),
        crate::AccessRights::READ_ADD_WRITE,
    )
}
