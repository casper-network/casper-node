use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes};

use super::binary_response::BinaryResponse;

pub struct BinaryResponseAndRequest {
    /// The original request (as serialized bytes).
    pub original_request: Vec<u8>,
    /// The response.
    pub response: BinaryResponse,
}

impl BinaryResponseAndRequest {
    pub fn new(data: BinaryResponse, original_request: &[u8]) -> Self {
        Self {
            original_request: original_request.to_vec(),
            response: data,
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
