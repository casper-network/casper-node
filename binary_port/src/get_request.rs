use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest,
};

use crate::state_request::GlobalStateRequest;

#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;

const RECORD_TAG: u8 = 0;
const INFORMATION_TAG: u8 = 1;
const STATE_TAG: u8 = 2;
const TRIE_TAG: u8 = 3;

/// A request to get data from the node.
#[derive(Clone, Debug, PartialEq)]
pub enum GetRequest {
    /// Retrieves a record from the node.
    Record {
        /// Type tag of the record to retrieve.
        record_type_tag: u16,
        /// Key encoded into bytes.
        key: Vec<u8>,
    },
    /// Retrieves information from the node.
    Information {
        /// Type tag of the information to retrieve.
        info_type_tag: u16,
        /// Key encoded into bytes.
        key: Vec<u8>,
    },
    /// Retrieves data from the global state.
    State(Box<GlobalStateRequest>),
    /// Get a trie by its Digest.
    Trie {
        /// A trie key.
        trie_key: Digest,
    },
}

impl GetRequest {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..4) {
            0 => GetRequest::Record {
                record_type_tag: rng.gen(),
                key: rng.random_vec(16..32),
            },
            1 => GetRequest::Information {
                info_type_tag: rng.gen(),
                key: rng.random_vec(16..32),
            },
            2 => GetRequest::State(Box::new(GlobalStateRequest::random(rng))),
            3 => GetRequest::Trie {
                trie_key: Digest::random(rng),
            },
            _ => unreachable!(),
        }
    }
}

impl ToBytes for GetRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GetRequest::Record {
                record_type_tag,
                key,
            } => {
                RECORD_TAG.write_bytes(writer)?;
                record_type_tag.write_bytes(writer)?;
                key.write_bytes(writer)
            }
            GetRequest::Information { info_type_tag, key } => {
                INFORMATION_TAG.write_bytes(writer)?;
                info_type_tag.write_bytes(writer)?;
                key.write_bytes(writer)
            }
            GetRequest::State(req) => {
                STATE_TAG.write_bytes(writer)?;
                req.write_bytes(writer)
            }
            GetRequest::Trie { trie_key } => {
                TRIE_TAG.write_bytes(writer)?;
                trie_key.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GetRequest::Record {
                    record_type_tag,
                    key,
                } => record_type_tag.serialized_length() + key.serialized_length(),
                GetRequest::Information { info_type_tag, key } => {
                    info_type_tag.serialized_length() + key.serialized_length()
                }
                GetRequest::State(req) => req.serialized_length(),
                GetRequest::Trie { trie_key } => trie_key.serialized_length(),
            }
    }
}

impl FromBytes for GetRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = FromBytes::from_bytes(bytes)?;
        match tag {
            RECORD_TAG => {
                let (record_type_tag, remainder) = FromBytes::from_bytes(remainder)?;
                let (key, remainder) = Bytes::from_bytes(remainder)?;
                Ok((
                    GetRequest::Record {
                        record_type_tag,
                        key: key.into(),
                    },
                    remainder,
                ))
            }
            INFORMATION_TAG => {
                let (info_type_tag, remainder) = FromBytes::from_bytes(remainder)?;
                let (key, remainder) = Bytes::from_bytes(remainder)?;
                Ok((
                    GetRequest::Information {
                        info_type_tag,
                        key: key.into(),
                    },
                    remainder,
                ))
            }
            STATE_TAG => {
                let (req, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((GetRequest::State(Box::new(req)), remainder))
            }
            TRIE_TAG => {
                let (trie_key, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((GetRequest::Trie { trie_key }, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = GetRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
