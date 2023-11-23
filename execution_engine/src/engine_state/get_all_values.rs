//! Support for obtaining all values under the given key tag.
use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, KeyTag,
};

/// Represents a request to obtain all values under the given key tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetAllValuesRequest {
    state_hash: Digest,
    key_tag: KeyTag,
}

impl GetAllValuesRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest, key_tag: KeyTag) -> Self {
        Self {
            state_hash,
            key_tag,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns key tag.
    pub fn key_tag(&self) -> KeyTag {
        self.key_tag
    }
}

const ROOT_NOT_FOUND_TAG: u8 = 0;
const SUCCESS_TAG: u8 = 1;
const SERIALIZATION_ERROR_TAG: u8 = 2;

/// Represents a result of a `get_all_values` request.
#[derive(Debug)]
pub enum GetAllValuesResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains values returned from the global state.
    Success {
        /// Current values.
        values: Vec<u8>,
    },
    /// Unable to serialize response.
    SerializationError,
}

impl ToBytes for GetAllValuesResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GetAllValuesResult::RootNotFound => ROOT_NOT_FOUND_TAG.write_bytes(writer),
            GetAllValuesResult::Success { values } => {
                SUCCESS_TAG.write_bytes(writer)?;
                values.write_bytes(writer)
            }
            GetAllValuesResult::SerializationError => SERIALIZATION_ERROR_TAG.write_bytes(writer),
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GetAllValuesResult::RootNotFound => 0,
                GetAllValuesResult::Success { values } => values.serialized_length(),
                GetAllValuesResult::SerializationError => 0,
            }
    }
}

impl FromBytes for GetAllValuesResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            SUCCESS_TAG => {
                let (values, remainder) = Bytes::from_bytes(remainder)?;
                Ok((
                    GetAllValuesResult::Success {
                        values: values.into(),
                    },
                    remainder,
                ))
            }
            ROOT_NOT_FOUND_TAG => Ok((GetAllValuesResult::RootNotFound, remainder)),
            SERIALIZATION_ERROR_TAG => Ok((GetAllValuesResult::SerializationError, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
