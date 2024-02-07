use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

/// The runtime used to execute a [`Transaction`].
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Runtime used to execute a Transaction.")
)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum TransactionRuntime {
    /// The Casper Version 1 Virtual Machine.
    VmCasperV1,
}

impl Display for TransactionRuntime {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionRuntime::VmCasperV1 => write!(formatter, "vm-casper-v1"),
        }
    }
}

impl ToBytes for TransactionRuntime {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        (*self as u8).write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for TransactionRuntime {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            v if v == TransactionRuntime::VmCasperV1 as u8 => {
                Ok((TransactionRuntime::VmCasperV1, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        bytesrepr::test_serialization_roundtrip(&TransactionRuntime::VmCasperV1);
    }
}
