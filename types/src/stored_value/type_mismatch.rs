use alloc::{string::String, vec::Vec};
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// An error struct representing a type mismatch in [`StoredValue`](crate::StoredValue) operations.
#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct TypeMismatch {
    /// The name of the expected type.
    expected: String,
    /// The actual type found.
    found: String,
}

impl TypeMismatch {
    /// Creates a new `TypeMismatch`.
    pub fn new(expected: String, found: String) -> TypeMismatch {
        TypeMismatch { expected, found }
    }
}

impl Display for TypeMismatch {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "Type mismatch. Expected {} but found {}.",
            self.expected, self.found
        )
    }
}

impl ToBytes for TypeMismatch {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.expected.write_bytes(writer)?;
        self.found.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.expected.serialized_length() + self.found.serialized_length()
    }
}

impl FromBytes for TypeMismatch {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (expected, remainder) = String::from_bytes(bytes)?;
        let (found, remainder) = String::from_bytes(remainder)?;
        Ok((TypeMismatch { expected, found }, remainder))
    }
}

#[cfg(feature = "std")]
impl StdError for TypeMismatch {}
