use super::PROTOCOL_VERSION;
#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    ErrorCode, PayloadType,
};
use alloc::vec::Vec;
#[cfg(test)]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;

/// Header of the binary response.
#[derive(Debug, PartialEq)]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The header of the binary response.")
)]
pub struct BinaryResponseHeader {
    protocol_version: u8,
    error: u8,
    returned_data_type: Option<PayloadType>,
}

impl BinaryResponseHeader {
    /// Creates new binary response header representing success.
    pub fn new(returned_data_type: Option<PayloadType>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            error: ErrorCode::NoError as u8,
            returned_data_type,
        }
    }

    /// Creates new binary response header representing error.
    pub fn new_error(error: ErrorCode) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            error: error as u8,
            returned_data_type: None,
        }
    }

    /// Returns the type of the returned data.
    pub fn returned_data_type(&self) -> Option<PayloadType> {
        self.returned_data_type
    }

    /// Returns the error code.
    pub fn error_code(&self) -> u8 {
        self.error
    }

    /// Returns true if the response represents success.
    pub fn is_success(&self) -> bool {
        self.error == ErrorCode::NoError as u8
    }

    /// Returns true if the response represents error.
    pub fn is_not_found(&self) -> bool {
        self.error == ErrorCode::NotFound as u8
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        let protocol_version = rng.gen();
        let error = rng.gen();
        let returned_data_type = if rng.gen() {
            None
        } else {
            Some(PayloadType::random(rng))
        };

        BinaryResponseHeader {
            protocol_version,
            error,
            returned_data_type,
        }
    }
}

impl ToBytes for BinaryResponseHeader {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let Self {
            protocol_version,
            error,
            returned_data_type,
        } = self;

        protocol_version.write_bytes(writer)?;
        error.write_bytes(writer)?;
        returned_data_type.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.protocol_version.serialized_length()
            + self.error.serialized_length()
            + self.returned_data_type.serialized_length()
    }
}

impl FromBytes for BinaryResponseHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version, remainder) = FromBytes::from_bytes(bytes)?;
        let (error, remainder) = FromBytes::from_bytes(remainder)?;
        let (payload_type, remainder) = FromBytes::from_bytes(remainder)?;

        Ok((
            BinaryResponseHeader {
                protocol_version,
                error,
                returned_data_type: payload_type,
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

        let val = BinaryResponseHeader::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
