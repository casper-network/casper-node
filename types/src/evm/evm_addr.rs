use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Address, Hash, StorageAddr};
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

/// EVM global-state address.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EvmAddr {
    /// EVM account identity address.
    Account(Address),
    /// EVM contract bytecode address, keyed by code hash.
    ByteCode(Hash),
    /// EVM contract storage slot address.
    Storage(StorageAddr),
    /// EVM account nonce address.
    Nonce(Address),
    /// EVM account code hash address.
    CodeHash(Address),
}

impl EvmAddr {
    /// Inner tag for EVM account addresses.
    pub const ACCOUNT_TAG: u8 = 0;
    /// Inner tag for EVM bytecode addresses.
    pub const BYTE_CODE_TAG: u8 = 1;
    /// Inner tag for EVM storage addresses.
    pub const STORAGE_TAG: u8 = 2;
    /// Inner tag for EVM account nonce addresses.
    pub const NONCE_TAG: u8 = 3;
    /// Inner tag for EVM account code hash addresses.
    pub const CODE_HASH_TAG: u8 = 4;
}

impl ToBytes for EvmAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut bytes = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut bytes)?;
        Ok(bytes)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                EvmAddr::Account(address) => address.serialized_length(),
                EvmAddr::ByteCode(hash) => hash.serialized_length(),
                EvmAddr::Storage(addr) => addr.serialized_length(),
                EvmAddr::Nonce(address) => address.serialized_length(),
                EvmAddr::CodeHash(address) => address.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            EvmAddr::Account(address) => {
                writer.push(Self::ACCOUNT_TAG);
                address.write_bytes(writer)
            }
            EvmAddr::ByteCode(hash) => {
                writer.push(Self::BYTE_CODE_TAG);
                hash.write_bytes(writer)
            }
            EvmAddr::Storage(addr) => {
                writer.push(Self::STORAGE_TAG);
                addr.write_bytes(writer)
            }
            EvmAddr::Nonce(address) => {
                writer.push(Self::NONCE_TAG);
                address.write_bytes(writer)
            }
            EvmAddr::CodeHash(address) => {
                writer.push(Self::CODE_HASH_TAG);
                address.write_bytes(writer)
            }
        }
    }
}

impl FromBytes for EvmAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            Self::ACCOUNT_TAG => Address::from_bytes(remainder)
                .map(|(address, remainder)| (EvmAddr::Account(address), remainder)),
            Self::BYTE_CODE_TAG => Hash::from_bytes(remainder)
                .map(|(hash, remainder)| (EvmAddr::ByteCode(hash), remainder)),
            Self::STORAGE_TAG => StorageAddr::from_bytes(remainder)
                .map(|(addr, remainder)| (EvmAddr::Storage(addr), remainder)),
            Self::NONCE_TAG => Address::from_bytes(remainder)
                .map(|(address, remainder)| (EvmAddr::Nonce(address), remainder)),
            Self::CODE_HASH_TAG => Address::from_bytes(remainder)
                .map(|(address, remainder)| (EvmAddr::CodeHash(address), remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl rand::distributions::Distribution<EvmAddr> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> EvmAddr {
        match rng.gen_range(0..=4) {
            0 => EvmAddr::Account(Address::new(rng.gen())),
            1 => EvmAddr::ByteCode(Hash::new(rng.gen())),
            2 => EvmAddr::Storage(StorageAddr::new(Address::new(rng.gen()), rng.gen())),
            3 => EvmAddr::Nonce(Address::new(rng.gen())),
            4 => EvmAddr::CodeHash(Address::new(rng.gen())),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr, U256};

    #[test]
    fn bytesrepr_roundtrip() {
        let address = Address::new([1; 20]);
        let hash = Hash::new([2; 32]);

        bytesrepr::test_serialization_roundtrip(&EvmAddr::Account(address));
        bytesrepr::test_serialization_roundtrip(&EvmAddr::ByteCode(hash));
        bytesrepr::test_serialization_roundtrip(&EvmAddr::Storage(StorageAddr::new(
            address,
            U256::MAX,
        )));
        bytesrepr::test_serialization_roundtrip(&EvmAddr::Nonce(address));
        bytesrepr::test_serialization_roundtrip(&EvmAddr::CodeHash(address));
    }
}
