//! Types for the `State::AllValues` request.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    StoredValue,
};

const ROOT_NOT_FOUND_TAG: u8 = 0;
const SUCCESS_TAG: u8 = 1;

/// Represents a result of a `get_all_values` request.
#[derive(Debug)]
pub enum GetAllValuesResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains values returned from the global state.
    Success {
        /// Current values.
        values: Vec<StoredValue>,
    },
}

impl ToBytes for GetAllValuesResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GetAllValuesResult::RootNotFound => ROOT_NOT_FOUND_TAG.write_bytes(writer),
            GetAllValuesResult::Success { values } => {
                SUCCESS_TAG.write_bytes(writer)?;
                values.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GetAllValuesResult::RootNotFound => 0,
                GetAllValuesResult::Success { values } => values.serialized_length(),
            }
    }
}

impl FromBytes for GetAllValuesResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            SUCCESS_TAG => {
                let (values, remainder) = <Vec<StoredValue>>::from_bytes(remainder)?;
                Ok((GetAllValuesResult::Success { values }, remainder))
            }
            ROOT_NOT_FOUND_TAG => Ok((GetAllValuesResult::RootNotFound, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
