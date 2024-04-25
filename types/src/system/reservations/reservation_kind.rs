use crate::{
    bytesrepr,
    bytesrepr::{Bytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest,
};
use alloc::vec::Vec;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Container for bytes recording location, type and data for a gas reservation
#[derive(Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct ReservationKind {
    receipt: Digest,
    reservation_kind: u8,
    reservation_data: Bytes,
}

impl ToBytes for ReservationKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.receipt.serialized_length()
            + U8_SERIALIZED_LENGTH
            + self.reservation_data.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.receipt.write_bytes(writer)?;
        self.reservation_kind.write_bytes(writer)?;
        self.reservation_data.write_bytes(writer)?;
        Ok(())
    }
}
