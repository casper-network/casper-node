use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLValue,
};

/// Type disambiguating between the formatting of the returned data.
#[derive(PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[non_exhaustive]
pub enum RetValue {
    /// The returned data is CLValue.
    CLValue(CLValue),
    /// The returned data is serialized bytes.
    Bytes(Bytes),
}

impl ToBytes for RetValue {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            RetValue::CLValue(bytes) => {
                (RetValueTag::CLValue as u8).write_bytes(writer)?;
                bytes.write_bytes(writer)
            }
            RetValue::Bytes(bytes) => {
                (RetValueTag::Bytes as u8).write_bytes(writer)?;
                bytes.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                RetValue::CLValue(bytes) => bytes.serialized_length(),
                RetValue::Bytes(bytes) => bytes.serialized_length(),
            }
    }
}

impl FromBytes for RetValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            tag if tag == RetValueTag::CLValue as u8 => {
                let (value, remainder) = CLValue::from_bytes(remainder)?;
                Ok((RetValue::CLValue(value), remainder))
            }
            tag if tag == RetValueTag::Bytes as u8 => {
                let (bytes, remainder) = Bytes::from_bytes(remainder)?;
                Ok((RetValue::Bytes(bytes), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[repr(u8)]
enum RetValueTag {
    CLValue = 0,
    Bytes = 1,
}
