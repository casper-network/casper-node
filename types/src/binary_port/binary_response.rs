//! The binary response.

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    SemVer,
};
use alloc::vec::Vec;

#[cfg(test)]
use crate::testing::TestRng;

use super::{
    binary_response_header::BinaryResponseHeader,
    db_id::DbId,
    payload_type::{PayloadEntity, PayloadType},
    DbRawBytesSpec, ErrorCode,
};

/// The response use in the binary port protocol.
#[derive(Debug, PartialEq)]
pub struct BinaryResponse {
    /// Header of the binary response.
    header: BinaryResponseHeader,
    /// The response.
    payload: Vec<u8>,
}

impl BinaryResponse {
    /// Creates new empty binary response.
    pub fn new_empty() -> Self {
        Self {
            header: BinaryResponseHeader::new(None),
            payload: vec![],
        }
    }

    /// Creates new binary response with error code.
    pub fn new_error(error: ErrorCode) -> Self {
        BinaryResponse {
            header: BinaryResponseHeader::new_error(error),
            payload: vec![],
        }
    }

    /// Creates new binary response from raw DB bytes.
    pub fn from_db_raw_bytes(db_id: DbId, spec: Option<DbRawBytesSpec>) -> Self {
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

    /// Creates a new binary response from a value.
    pub fn from_value<V>(val: V) -> Self
    where
        V: ToBytes + PayloadEntity,
    {
        BinaryResponse {
            payload: ToBytes::to_bytes(&val).unwrap(),
            header: BinaryResponseHeader::new(Some(V::PAYLOAD_TYPE)),
        }
    }

    /// Creates a new binary response from an optional value.
    pub fn from_option<V>(opt: Option<V>) -> Self
    where
        V: ToBytes + PayloadEntity,
    {
        match opt {
            Some(val) => Self::from_value(val),
            None => Self::new_empty(),
        }
    }

    /// Creates a binary response with specified value and protocol version.
    #[cfg(any(feature = "testing", test))]
    pub fn from_value_with_protocol_version<V>(val: V, ver: SemVer) -> Self
    where
        V: ToBytes + PayloadEntity,
    {
        Self {
            header: BinaryResponseHeader::new_with_protocol_version(Some(V::PAYLOAD_TYPE), ver),
            payload: ToBytes::to_bytes(&val).unwrap(),
        }
    }

    /// Returns true if response is success.
    pub fn is_success(&self) -> bool {
        self.header.is_success()
    }

    /// Returns the error code.
    pub fn error_code(&self) -> u8 {
        self.header.error_code()
    }

    /// Returns the payload type of the response.
    pub fn returned_data_type_tag(&self) -> Option<u8> {
        self.header.returned_data_type_tag()
    }

    /// Returns true if the response means that data has not been found.
    pub fn is_not_found(&self) -> bool {
        self.header.is_not_found()
    }

    /// Returns the payload.
    pub fn payload(&self) -> &[u8] {
        self.payload.as_ref()
    }

    /// Returns the protocol version.
    pub fn protocol_version(&self) -> SemVer {
        self.header.protocol_version()
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            header: BinaryResponseHeader::random(rng),
            payload: rng.random_vec(64..128),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = BinaryResponse::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
