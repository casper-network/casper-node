use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLValueError, StoredValueTypeMismatch,
};

/// Error type for applying and combining transforms.
///
/// A `TypeMismatch` occurs when a transform cannot be applied because the types are not compatible
/// (e.g. trying to add a number to a string).
#[derive(PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[non_exhaustive]
pub enum TransformError {
    /// Error while (de)serializing data.
    Serialization(bytesrepr::Error),
    /// Type mismatch error.
    TypeMismatch(StoredValueTypeMismatch),
    /// Type no longer supported.
    Deprecated,
}

impl Display for TransformError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransformError::Serialization(error) => {
                write!(formatter, "{}", error)
            }
            TransformError::TypeMismatch(error) => {
                write!(formatter, "{}", error)
            }
            TransformError::Deprecated => {
                write!(formatter, "type no longer supported")
            }
        }
    }
}

impl From<StoredValueTypeMismatch> for TransformError {
    fn from(error: StoredValueTypeMismatch) -> Self {
        TransformError::TypeMismatch(error)
    }
}

impl From<CLValueError> for TransformError {
    fn from(cl_value_error: CLValueError) -> TransformError {
        match cl_value_error {
            CLValueError::Serialization(error) => TransformError::Serialization(error),
            CLValueError::Type(cl_type_mismatch) => {
                let expected = format!("{:?}", cl_type_mismatch.expected);
                let found = format!("{:?}", cl_type_mismatch.found);
                let type_mismatch = StoredValueTypeMismatch::new(expected, found);
                TransformError::TypeMismatch(type_mismatch)
            }
        }
    }
}

impl ToBytes for TransformError {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransformError::Serialization(error) => {
                (TransformErrorTag::Serialization as u8).write_bytes(writer)?;
                error.write_bytes(writer)
            }
            TransformError::TypeMismatch(error) => {
                (TransformErrorTag::TypeMismatch as u8).write_bytes(writer)?;
                error.write_bytes(writer)
            }
            TransformError::Deprecated => (TransformErrorTag::Deprecated as u8).write_bytes(writer),
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
                TransformError::Serialization(error) => error.serialized_length(),
                TransformError::TypeMismatch(error) => error.serialized_length(),
                TransformError::Deprecated => 0,
            }
    }
}

impl FromBytes for TransformError {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            tag if tag == TransformErrorTag::Serialization as u8 => {
                let (error, remainder) = bytesrepr::Error::from_bytes(remainder)?;
                Ok((TransformError::Serialization(error), remainder))
            }
            tag if tag == TransformErrorTag::TypeMismatch as u8 => {
                let (error, remainder) = StoredValueTypeMismatch::from_bytes(remainder)?;
                Ok((TransformError::TypeMismatch(error), remainder))
            }
            tag if tag == TransformErrorTag::Deprecated as u8 => {
                Ok((TransformError::Deprecated, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(feature = "std")]
impl StdError for TransformError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            TransformError::Serialization(error) => Some(error),
            TransformError::TypeMismatch(_) | TransformError::Deprecated => None,
        }
    }
}

#[repr(u8)]
enum TransformErrorTag {
    Serialization = 0,
    TypeMismatch = 1,
    Deprecated = 2,
}
