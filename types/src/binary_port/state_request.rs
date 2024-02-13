use alloc::string::String;
use alloc::vec::Vec;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, GlobalStateIdentifier, Key, KeyTag,
};

const ITEM_TAG: u8 = 0;
const ALL_ITEMS_TAG: u8 = 1;
const TRIE_TAG: u8 = 2;

/// A request to get data from the global state.
#[derive(Clone, Debug, PartialEq)]
pub enum GlobalStateRequest {
    /// Gets an item from the global state.
    Item {
        /// Global state identifier, `None` means "latest block state".
        state_identifier: Option<GlobalStateIdentifier>,
        /// Key under which data is stored.
        base_key: Key,
        /// Path under which the value is stored.
        path: Vec<String>,
    },
    /// Get all items under the given key tag.
    AllItems {
        /// Global state identifier, `None` means "latest block state".
        state_identifier: Option<GlobalStateIdentifier>,
        /// Key tag
        key_tag: KeyTag,
    },
    /// Get a trie by its Digest.
    Trie {
        /// A trie key.
        trie_key: Digest,
    },
}

impl GlobalStateRequest {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..5) {
            0 => {
                let path_count = rng.gen_range(10..20);
                let state_identifier = if rng.gen() {
                    Some(GlobalStateIdentifier::random(rng))
                } else {
                    None
                };
                GlobalStateRequest::Item {
                    state_identifier,
                    base_key: rng.gen(),
                    path: std::iter::repeat_with(|| rng.random_string(32..64))
                        .take(path_count)
                        .collect(),
                }
            }
            1 => {
                let state_identifier = if rng.gen() {
                    Some(GlobalStateIdentifier::random(rng))
                } else {
                    None
                };
                GlobalStateRequest::AllItems {
                    state_identifier,
                    key_tag: KeyTag::random(rng),
                }
            }
            2 => GlobalStateRequest::Trie {
                trie_key: Digest::random(rng),
            },
            _ => unreachable!(),
        }
    }
}

impl ToBytes for GlobalStateRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GlobalStateRequest::Item {
                state_identifier,
                base_key,
                path,
            } => {
                ITEM_TAG.write_bytes(writer)?;
                state_identifier.write_bytes(writer)?;
                base_key.write_bytes(writer)?;
                path.write_bytes(writer)
            }
            GlobalStateRequest::AllItems {
                state_identifier,
                key_tag,
            } => {
                ALL_ITEMS_TAG.write_bytes(writer)?;
                state_identifier.write_bytes(writer)?;
                key_tag.write_bytes(writer)
            }
            GlobalStateRequest::Trie { trie_key } => {
                TRIE_TAG.write_bytes(writer)?;
                trie_key.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GlobalStateRequest::Item {
                    state_identifier,
                    base_key,
                    path,
                } => {
                    state_identifier.serialized_length()
                        + base_key.serialized_length()
                        + path.serialized_length()
                }
                GlobalStateRequest::AllItems {
                    state_identifier,
                    key_tag,
                } => state_identifier.serialized_length() + key_tag.serialized_length(),
                GlobalStateRequest::Trie { trie_key } => trie_key.serialized_length(),
            }
    }
}

impl FromBytes for GlobalStateRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ITEM_TAG => {
                let (state_identifier, remainder) = FromBytes::from_bytes(remainder)?;
                let (base_key, remainder) = FromBytes::from_bytes(remainder)?;
                let (path, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateRequest::Item {
                        state_identifier,
                        base_key,
                        path,
                    },
                    remainder,
                ))
            }
            ALL_ITEMS_TAG => {
                let (state_identifier, remainder) = FromBytes::from_bytes(remainder)?;
                let (key_tag, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateRequest::AllItems {
                        state_identifier,
                        key_tag,
                    },
                    remainder,
                ))
            }
            TRIE_TAG => {
                let (trie_key, remainder) = Digest::from_bytes(remainder)?;
                Ok((GlobalStateRequest::Trie { trie_key }, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = GlobalStateRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
