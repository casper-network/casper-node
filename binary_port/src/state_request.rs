#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockIdentifier, Digest, GlobalStateIdentifier, Key, KeyTag,
};

use crate::PurseIdentifier;

use super::dictionary_item_identifier::DictionaryItemIdentifier;

const ITEM_TAG: u8 = 0;
const ALL_ITEMS_TAG: u8 = 1;
const TRIE_TAG: u8 = 2;
const DICTIONARY_ITEM_TAG: u8 = 3;
const BALANCE_BY_BLOCK_TAG: u8 = 4;
const BALANCE_BY_STATE_ROOT_TAG: u8 = 5;

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
    /// Get a dictionary item by its identifier.
    DictionaryItem {
        /// Global state identifier, `None` means "latest block state".
        state_identifier: Option<GlobalStateIdentifier>,
        /// Dictionary item identifier.
        identifier: DictionaryItemIdentifier,
    },
    /// Get balance by block and purse.
    BalanceByBlock {
        /// Block identifier, `None` means "latest block".
        block_identifier: Option<BlockIdentifier>,
        /// Purse identifier.
        purse_identifier: PurseIdentifier,
    },
    /// Get balance by state root and purse.
    BalanceByStateRoot {
        /// Global state identifier, `None` means "latest block state".
        state_identifier: Option<GlobalStateIdentifier>,
        /// Purse identifier.
        purse_identifier: PurseIdentifier,
    },
}

impl GlobalStateRequest {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match TestRng::gen_range(rng, 0..5) {
            ITEM_TAG => {
                let path_count = rng.gen_range(10..20);
                GlobalStateRequest::Item {
                    state_identifier: rng
                        .gen::<bool>()
                        .then(|| GlobalStateIdentifier::random(rng)),
                    base_key: rng.gen(),
                    path: std::iter::repeat_with(|| rng.random_string(32..64))
                        .take(path_count)
                        .collect(),
                }
            }
            ALL_ITEMS_TAG => GlobalStateRequest::AllItems {
                state_identifier: rng
                    .gen::<bool>()
                    .then(|| GlobalStateIdentifier::random(rng)),
                key_tag: KeyTag::random(rng),
            },
            TRIE_TAG => GlobalStateRequest::Trie {
                trie_key: Digest::random(rng),
            },
            DICTIONARY_ITEM_TAG => GlobalStateRequest::DictionaryItem {
                state_identifier: rng
                    .gen::<bool>()
                    .then(|| GlobalStateIdentifier::random(rng)),
                identifier: DictionaryItemIdentifier::random(rng),
            },
            BALANCE_BY_BLOCK_TAG => GlobalStateRequest::BalanceByBlock {
                block_identifier: rng.gen::<bool>().then(|| BlockIdentifier::random(rng)),
                purse_identifier: PurseIdentifier::random(rng),
            },
            BALANCE_BY_STATE_ROOT_TAG => GlobalStateRequest::BalanceByStateRoot {
                state_identifier: rng
                    .gen::<bool>()
                    .then(|| GlobalStateIdentifier::random(rng)),
                purse_identifier: PurseIdentifier::random(rng),
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
            GlobalStateRequest::DictionaryItem {
                state_identifier,
                identifier,
            } => {
                DICTIONARY_ITEM_TAG.write_bytes(writer)?;
                state_identifier.write_bytes(writer)?;
                identifier.write_bytes(writer)
            }
            GlobalStateRequest::BalanceByBlock {
                block_identifier,
                purse_identifier,
            } => {
                BALANCE_BY_BLOCK_TAG.write_bytes(writer)?;
                block_identifier.write_bytes(writer)?;
                purse_identifier.write_bytes(writer)
            }
            GlobalStateRequest::BalanceByStateRoot {
                state_identifier,
                purse_identifier,
            } => {
                BALANCE_BY_STATE_ROOT_TAG.write_bytes(writer)?;
                state_identifier.write_bytes(writer)?;
                purse_identifier.write_bytes(writer)
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
                GlobalStateRequest::DictionaryItem {
                    state_identifier,
                    identifier,
                } => state_identifier.serialized_length() + identifier.serialized_length(),
                GlobalStateRequest::BalanceByBlock {
                    block_identifier,
                    purse_identifier,
                } => block_identifier.serialized_length() + purse_identifier.serialized_length(),
                GlobalStateRequest::BalanceByStateRoot {
                    state_identifier,
                    purse_identifier,
                } => state_identifier.serialized_length() + purse_identifier.serialized_length(),
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
            DICTIONARY_ITEM_TAG => {
                let (state_identifier, remainder) = FromBytes::from_bytes(remainder)?;
                let (identifier, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateRequest::DictionaryItem {
                        state_identifier,
                        identifier,
                    },
                    remainder,
                ))
            }
            BALANCE_BY_BLOCK_TAG => {
                let (block_identifier, remainder) = FromBytes::from_bytes(remainder)?;
                let (purse_identifier, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateRequest::BalanceByBlock {
                        block_identifier,
                        purse_identifier,
                    },
                    remainder,
                ))
            }
            BALANCE_BY_STATE_ROOT_TAG => {
                let (state_identifier, remainder) = FromBytes::from_bytes(remainder)?;
                let (purse_identifier, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateRequest::BalanceByStateRoot {
                        state_identifier,
                        purse_identifier,
                    },
                    remainder,
                ))
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

        let val = GlobalStateRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
