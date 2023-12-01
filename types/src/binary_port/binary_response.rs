//! The binary response.

use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes};

use super::{
    binary_request::BinaryRequest, db_id::DbId, error::Error, payload_type::PayloadType,
    DbRawBytesSpec, PROTOCOL_VERSION,
};

/// Header of the binary response.
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
            error: Error::NoError as u8,
            returned_data_type,
        }
    }

    /// Creates new binary response header representing error.
    pub fn new_error(error: Error) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            error: error as u8,
            returned_data_type: None,
        }
    }

    /// Returns the type of the returned data.
    pub fn returned_data_type(&self) -> Option<&PayloadType> {
        self.returned_data_type.as_ref()
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

/// The response use in the binary port protocol.
pub struct BinaryResponse {
    /// Header of the binary response.
    pub header: BinaryResponseHeader,
    /// The original request (as serialized bytes).
    pub original_request: Vec<u8>,
    /// The payload.
    pub payload: Vec<u8>,
}

// We'll never be returning Ok(None), it'll always be Ok(Some(...)) pushed through Juliet

impl BinaryResponse {
    pub fn new_error(error: Error, binary_request: BinaryRequest) -> Self {
        BinaryResponse {
            header: BinaryResponseHeader::new_error(error),
            original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
            payload: vec![],
        }
    }

    pub fn from_db_raw_bytes(
        db_id: &DbId,
        binary_request: BinaryRequest,
        spec: Option<DbRawBytesSpec>,
    ) -> Self {
        match spec {
            Some(DbRawBytesSpec {
                is_legacy,
                raw_bytes,
            }) => BinaryResponse {
                header: BinaryResponseHeader::new(Some(PayloadType::new_from_db_id(
                    db_id, is_legacy,
                ))),
                original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
                payload: raw_bytes,
            },
            None => BinaryResponse {
                header: BinaryResponseHeader::new_error(Error::NotFound),
                original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
                payload: vec![],
            },
        }
    }

    // TODO[RC]: Can we prevent V from being an Option here?
    pub fn from_value<V>(binary_request: BinaryRequest, val: V) -> Self
    where
        V: ToBytes,
        V: Into<PayloadType>,
    {
        BinaryResponse {
            payload: ToBytes::to_bytes(&val).unwrap(),
            header: BinaryResponseHeader::new(Some(val.into())),
            original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
        }
    }

    pub fn from_opt<V>(binary_request: BinaryRequest, val: Option<V>) -> Self
    where
        V: ToBytes,
        V: Into<PayloadType>,
    {
        match val {
            Some(val) => Self::from_value(binary_request, val),
            None => BinaryResponse {
                payload: vec![],
                header: BinaryResponseHeader::new_error(Error::NotFound),
                original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
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
        let BinaryResponse {
            header,
            original_request,
            payload,
        } = self;

        header.write_bytes(writer)?;
        original_request.write_bytes(writer)?;
        payload.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.header.serialized_length()
            + self.original_request.serialized_length()
            + self.payload.serialized_length()
    }
}

impl FromBytes for BinaryResponse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (header, remainder) = FromBytes::from_bytes(bytes)?;
        let (original_request, remainder) = Bytes::from_bytes(remainder)?;
        let (payload, remainder) = Bytes::from_bytes(remainder)?;

        Ok((
            BinaryResponse {
                header,
                original_request: original_request.into(),
                payload: payload.into(),
            },
            remainder,
        ))
    }
}
