use super::dictionary_item_identifier::DictionaryItemIdentifier;
use crate::{KeyPrefix, PurseIdentifier};
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Key, KeyTag,
};
#[cfg(test)]
use rand::Rng;

const ITEM_TAG: u8 = 0;
const ALL_ITEMS_TAG: u8 = 1;
const DICTIONARY_ITEM_TAG: u8 = 2;
const BALANCE_TAG: u8 = 3;
const ITEMS_BY_PREFIX_TAG: u8 = 4;

/// A request to get data from the global state.
#[derive(Clone, Debug, PartialEq)]
pub enum GlobalStateEntityQualifier {
    /// Gets an item from the global state.
    Item {
        /// Key under which data is stored.
        base_key: Key,
        /// Path under which the value is stored.
        path: Vec<String>,
    },
    /// Get all items under the given key tag.
    AllItems {
        /// Key tag
        key_tag: KeyTag,
    },
    /// Get a dictionary item by its identifier.
    DictionaryItem {
        /// Dictionary item identifier.
        identifier: DictionaryItemIdentifier,
    },
    /// Get balance by state root and purse.
    Balance {
        /// Purse identifier.
        purse_identifier: PurseIdentifier,
    },
    ItemsByPrefix {
        /// Key prefix to search for.
        key_prefix: KeyPrefix,
    },
}

impl GlobalStateEntityQualifier {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        let gen_range = TestRng::gen_range(rng, 0..5);
        random_for_variant(gen_range, rng)
    }
}

#[cfg(test)]
fn random_for_variant(gen_range: u8, rng: &mut TestRng) -> GlobalStateEntityQualifier {
    match gen_range {
        ITEM_TAG => {
            let path_count = rng.gen_range(10..20);
            GlobalStateEntityQualifier::Item {
                base_key: rng.gen(),
                path: std::iter::repeat_with(|| rng.random_string(32..64))
                    .take(path_count)
                    .collect(),
            }
        }
        ALL_ITEMS_TAG => GlobalStateEntityQualifier::AllItems {
            key_tag: KeyTag::random(rng),
        },
        DICTIONARY_ITEM_TAG => GlobalStateEntityQualifier::DictionaryItem {
            identifier: DictionaryItemIdentifier::random(rng),
        },
        BALANCE_TAG => GlobalStateEntityQualifier::Balance {
            purse_identifier: PurseIdentifier::random(rng),
        },
        ITEMS_BY_PREFIX_TAG => GlobalStateEntityQualifier::ItemsByPrefix {
            key_prefix: KeyPrefix::random(rng),
        },
        _ => unreachable!(),
    }
}

impl ToBytes for GlobalStateEntityQualifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GlobalStateEntityQualifier::Item { base_key, path } => {
                ITEM_TAG.write_bytes(writer)?;
                base_key.write_bytes(writer)?;
                path.write_bytes(writer)
            }
            GlobalStateEntityQualifier::AllItems { key_tag } => {
                ALL_ITEMS_TAG.write_bytes(writer)?;
                key_tag.write_bytes(writer)
            }
            GlobalStateEntityQualifier::DictionaryItem { identifier } => {
                DICTIONARY_ITEM_TAG.write_bytes(writer)?;
                identifier.write_bytes(writer)
            }
            GlobalStateEntityQualifier::Balance { purse_identifier } => {
                BALANCE_TAG.write_bytes(writer)?;
                purse_identifier.write_bytes(writer)
            }
            GlobalStateEntityQualifier::ItemsByPrefix { key_prefix } => {
                ITEMS_BY_PREFIX_TAG.write_bytes(writer)?;
                key_prefix.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GlobalStateEntityQualifier::Item { base_key, path } => {
                    base_key.serialized_length() + path.serialized_length()
                }
                GlobalStateEntityQualifier::AllItems { key_tag } => key_tag.serialized_length(),
                GlobalStateEntityQualifier::DictionaryItem { identifier } => {
                    identifier.serialized_length()
                }
                GlobalStateEntityQualifier::Balance { purse_identifier } => {
                    purse_identifier.serialized_length()
                }
                GlobalStateEntityQualifier::ItemsByPrefix { key_prefix } => {
                    key_prefix.serialized_length()
                }
            }
    }
}

impl FromBytes for GlobalStateEntityQualifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ITEM_TAG => {
                let (base_key, remainder) = FromBytes::from_bytes(remainder)?;
                let (path, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateEntityQualifier::Item { base_key, path },
                    remainder,
                ))
            }
            ALL_ITEMS_TAG => {
                let (key_tag, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((GlobalStateEntityQualifier::AllItems { key_tag }, remainder))
            }
            DICTIONARY_ITEM_TAG => {
                let (identifier, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateEntityQualifier::DictionaryItem { identifier },
                    remainder,
                ))
            }
            BALANCE_TAG => {
                let (purse_identifier, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateEntityQualifier::Balance { purse_identifier },
                    remainder,
                ))
            }
            ITEMS_BY_PREFIX_TAG => {
                let (key_prefix, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GlobalStateEntityQualifier::ItemsByPrefix { key_prefix },
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
        for i in 0..5 {
            let qualifier = random_for_variant(i, rng);
            bytesrepr::test_serialization_roundtrip(&qualifier);
        }
    }
}
