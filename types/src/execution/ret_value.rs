use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
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
    /// There was no returned data.
    Unit,
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
            RetValue::Unit => (RetValueTag::Unit as u8).write_bytes(writer),
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
                RetValue::Unit => 0,
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
            tag if tag == RetValueTag::Unit as u8 => Ok((RetValue::Unit, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl RetValue {
    /// Generates a random `RetValue`.
    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        match rng.gen_range(0..3u8) {
            0 => RetValue::Unit,
            1 => RetValue::CLValue(CLValue::from_t(rng.gen::<u64>()).unwrap()),
            _ => RetValue::Bytes(Bytes::from(rng.gen::<u64>().to_le_bytes().to_vec())),
        }
    }
}

#[repr(u8)]
enum RetValueTag {
    CLValue = 0,
    Bytes = 1,
    Unit = 2,
}
