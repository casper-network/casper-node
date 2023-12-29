use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes};

use super::{binary_response::BinaryResponse, payload_type::PayloadEntity};
use alloc::vec::Vec;

#[cfg(any(feature = "testing", test))]
use super::db_id::DbId;
#[cfg(test)]
use crate::testing::TestRng;

/// The binary response along with the original binary request attached.
#[derive(Debug, PartialEq)]
pub struct BinaryResponseAndRequest {
    /// The original request (as serialized bytes).
    original_request: Vec<u8>,
    /// The response.
    response: BinaryResponse,
}

impl BinaryResponseAndRequest {
    /// Creates new binary response with the original request attached.
    pub fn new(data: BinaryResponse, original_request: &[u8]) -> Self {
        Self {
            original_request: original_request.to_vec(),
            response: data,
        }
    }

    /// Returns a new binary response with specified data and no original request.
    #[cfg(any(feature = "testing", test))]
    pub fn new_test_response<A: PayloadEntity + ToBytes>(
        db_id: DbId,
        data: &A,
    ) -> BinaryResponseAndRequest {
        use super::DbRawBytesSpec;

        let response = BinaryResponse::from_db_raw_bytes(
            db_id,
            Some(DbRawBytesSpec::new_current(&data.to_bytes().unwrap())),
        );
        Self::new(response, &[])
    }

    /// Returns a new binary response with specified legacy data and no original request.
    #[cfg(any(feature = "testing", test))]
    pub fn new_legacy_test_response<A: PayloadEntity + serde::Serialize>(
        db_id: DbId,
        data: &A,
    ) -> BinaryResponseAndRequest {
        use super::DbRawBytesSpec;

        let response = BinaryResponse::from_db_raw_bytes(
            db_id,
            Some(DbRawBytesSpec::new_legacy(
                &bincode::serialize(data).unwrap(),
            )),
        );
        Self::new(response, &[])
    }

    /// Returns true if response is success.
    pub fn is_success(&self) -> bool {
        self.response.is_success()
    }

    /// Returns the error code.
    pub fn error_code(&self) -> u8 {
        self.response.error_code()
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            original_request: rng.random_vec(64..128),
            response: BinaryResponse::random(rng),
        }
    }
}

impl ToBytes for BinaryResponseAndRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let BinaryResponseAndRequest {
            original_request,
            response,
        } = self;

        original_request.write_bytes(writer)?;
        response.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.original_request.serialized_length() + self.response.serialized_length()
    }
}

impl FromBytes for BinaryResponseAndRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (original_request, remainder) = Bytes::from_bytes(bytes)?;
        let (response, remainder) = FromBytes::from_bytes(remainder)?;

        Ok((
            BinaryResponseAndRequest {
                original_request: original_request.into(),
                response,
            },
            remainder,
        ))
    }
}

impl From<BinaryResponseAndRequest> for BinaryResponse {
    fn from(response_and_request: BinaryResponseAndRequest) -> Self {
        let BinaryResponseAndRequest { response, .. } = response_and_request;
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = BinaryResponseAndRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
