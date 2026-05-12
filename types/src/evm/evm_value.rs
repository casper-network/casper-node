use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Account, StorageValue};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    ByteCode,
};

/// EVM value stored in global state.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum EvmValue {
    /// EVM account metadata.
    Account(Account),
    /// EVM contract bytecode.
    ByteCode(ByteCode),
    /// EVM contract storage value for one slot.
    Storage(StorageValue),
}

impl EvmValue {
    const ACCOUNT_TAG: u8 = 0;
    const BYTE_CODE_TAG: u8 = 1;
    const STORAGE_TAG: u8 = 2;

    /// Returns a short type name for diagnostics.
    pub fn type_name(&self) -> &'static str {
        match self {
            EvmValue::Account(_) => "EvmAccount",
            EvmValue::ByteCode(_) => "EvmByteCode",
            EvmValue::Storage(_) => "EvmStorage",
        }
    }
}

impl ToBytes for EvmValue {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut bytes = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut bytes)?;
        Ok(bytes)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                EvmValue::Account(account) => account.serialized_length(),
                EvmValue::ByteCode(byte_code) => byte_code.serialized_length(),
                EvmValue::Storage(value) => value.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            EvmValue::Account(account) => {
                writer.push(Self::ACCOUNT_TAG);
                account.write_bytes(writer)
            }
            EvmValue::ByteCode(byte_code) => {
                writer.push(Self::BYTE_CODE_TAG);
                byte_code.write_bytes(writer)
            }
            EvmValue::Storage(value) => {
                writer.push(Self::STORAGE_TAG);
                value.write_bytes(writer)
            }
        }
    }
}

impl FromBytes for EvmValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            Self::ACCOUNT_TAG => Account::from_bytes(remainder)
                .map(|(account, remainder)| (EvmValue::Account(account), remainder)),
            Self::BYTE_CODE_TAG => ByteCode::from_bytes(remainder)
                .map(|(byte_code, remainder)| (EvmValue::ByteCode(byte_code), remainder)),
            Self::STORAGE_TAG => StorageValue::from_bytes(remainder)
                .map(|(value, remainder)| (EvmValue::Storage(value), remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr, evm, testing::TestRng, ByteCodeKind, U256};
    use rand::Rng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let account = Account::new(1, evm::EMPTY_CODE_HASH, rng.gen());
        let byte_code = ByteCode::new(ByteCodeKind::EvmPrague, vec![0x60, 0x00]);
        let storage = StorageValue::new(U256::MAX);

        bytesrepr::test_serialization_roundtrip(&EvmValue::Account(account));
        bytesrepr::test_serialization_roundtrip(&EvmValue::ByteCode(byte_code));
        bytesrepr::test_serialization_roundtrip(&EvmValue::Storage(storage));
    }
}
