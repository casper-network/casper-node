//! The binary response.

use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes};
use alloc::vec::Vec;

use super::{
    binary_response_header::BinaryResponseHeader, db_id::DbId, payload_type::PayloadType,
    DbRawBytesSpec, ErrorCode,
};

/// The response use in the binary port protocol.
pub struct BinaryResponse {
    /// Header of the binary response.
    pub header: BinaryResponseHeader,
    /// The response.
    pub payload: Vec<u8>,
}

impl BinaryResponse {
    pub fn new_empty() -> Self {
        Self {
            header: BinaryResponseHeader::new(None),
            payload: vec![],
        }
    }

    pub fn new_error(error: ErrorCode) -> Self {
        BinaryResponse {
            header: BinaryResponseHeader::new_error(error),
            payload: vec![],
        }
    }

    pub fn from_db_raw_bytes(db_id: &DbId, spec: Option<DbRawBytesSpec>) -> Self {
        match spec {
            Some(DbRawBytesSpec {
                is_legacy,
                raw_bytes,
            }) => BinaryResponse {
                header: BinaryResponseHeader::new(Some(PayloadType::new_from_db_id(
                    db_id, is_legacy,
                ))),
                payload: raw_bytes,
            },
            None => BinaryResponse {
                header: BinaryResponseHeader::new_error(ErrorCode::NotFound),
                payload: vec![],
            },
        }
    }

    // TODO[RC]: Can we prevent V from being an Option here?
    pub fn from_value<V>(val: V) -> Self
    where
        V: ToBytes,
        V: Into<PayloadType>,
    {
        BinaryResponse {
            payload: ToBytes::to_bytes(&val).unwrap(),
            header: BinaryResponseHeader::new(Some(val.into())),
        }
    }

    pub fn from_opt<V>(val: Option<V>) -> Self
    where
        V: ToBytes,
        V: Into<PayloadType>,
    {
        match val {
            Some(val) => Self::from_value(val),
            None => BinaryResponse {
                payload: vec![],
                header: BinaryResponseHeader::new_error(ErrorCode::NotFound),
            },
        }
    }
}

impl ToBytes for BinaryResponse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let BinaryResponse { header, payload } = self;

        header.write_bytes(writer)?;
        payload.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.header.serialized_length() + self.payload.serialized_length()
    }
}

impl FromBytes for BinaryResponse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (header, remainder) = FromBytes::from_bytes(bytes)?;
        let (payload, remainder) = Bytes::from_bytes(remainder)?;

        Ok((
            BinaryResponse {
                header,
                payload: payload.into(),
            },
            remainder,
        ))
    }
}
