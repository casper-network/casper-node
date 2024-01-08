use super::BINARY_PROTOCOL_VERSION;
#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    ErrorCode, PayloadType, SemVer,
};
use alloc::vec::Vec;
#[cfg(test)]
use rand::Rng;

/// Header of the binary response.
#[derive(Debug, PartialEq)]
pub struct BinaryResponseHeader {
    protocol_version: SemVer,
    error: u8,
    returned_data_type_tag: Option<u8>,
}

impl BinaryResponseHeader {
    /// Creates new binary response header representing success.
    pub fn new(returned_data_type: Option<PayloadType>) -> Self {
        Self {
            protocol_version: BINARY_PROTOCOL_VERSION,
            error: ErrorCode::NoError as u8,
            returned_data_type_tag: returned_data_type.map(|ty| ty as u8),
        }
    }

    /// Creates new binary response header representing error.
    pub fn new_error(error: ErrorCode) -> Self {
        Self {
            protocol_version: BINARY_PROTOCOL_VERSION,
            error: error as u8,
            returned_data_type_tag: None,
        }
    }

    /// Creates an binary response header with specified protocol version.
    #[cfg(any(feature = "testing", test))]
    pub fn new_with_protocol_version(
        returned_data_type: Option<PayloadType>,
        protocol_version: SemVer,
    ) -> Self {
        Self {
            protocol_version,
            error: ErrorCode::NoError as u8,
            returned_data_type_tag: returned_data_type.map(|ty| ty as u8),
        }
    }

    /// Returns the type of the returned data.
    pub fn returned_data_type_tag(&self) -> Option<u8> {
        self.returned_data_type_tag
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

    /// Returns the protocol version.
    pub fn protocol_version(&self) -> SemVer {
        self.protocol_version
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        let protocol_version = SemVer::new(rng.gen(), rng.gen(), rng.gen());
        let error = rng.gen();
        let returned_data_type_tag = if rng.gen() { None } else { Some(rng.gen()) };

        BinaryResponseHeader {
            protocol_version,
            error,
            returned_data_type_tag,
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
            returned_data_type_tag,
        } = self;

        protocol_version.write_bytes(writer)?;
        error.write_bytes(writer)?;
        returned_data_type_tag.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.protocol_version.serialized_length()
            + self.error.serialized_length()
            + self.returned_data_type_tag.serialized_length()
    }
}

impl FromBytes for BinaryResponseHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version, remainder) = FromBytes::from_bytes(bytes)?;
        let (error, remainder) = FromBytes::from_bytes(remainder)?;
        let (returned_data_type_tag, remainder) = FromBytes::from_bytes(remainder)?;

        Ok((
            BinaryResponseHeader {
                protocol_version,
                error,
                returned_data_type_tag,
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
