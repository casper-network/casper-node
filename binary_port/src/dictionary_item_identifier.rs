#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;

use casper_types::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    DictionaryAddr, EntityAddr, HashAddr, URef,
};

const ACCOUNT_NAMED_KEY_TAG: u8 = 0;
const CONTRACT_NAMED_KEY_TAG: u8 = 1;
const ENTITY_NAMED_KEY_TAG: u8 = 2;
const UREF_TAG: u8 = 3;
const DICTIONARY_ITEM_TAG: u8 = 4;

/// Options for dictionary item lookups.
#[derive(Clone, Debug, PartialEq)]
pub enum DictionaryItemIdentifier {
    /// Lookup a dictionary item via an accounts named keys.
    AccountNamedKey {
        /// The account hash.
        hash: AccountHash,
        /// The named key under which the dictionary seed URef is stored.
        dictionary_name: String,
        /// The dictionary item key formatted as a string.
        dictionary_item_key: String,
    },
    /// Lookup a dictionary item via a contracts named keys.
    ContractNamedKey {
        /// The contract hash.
        hash: HashAddr,
        /// The named key under which the dictionary seed URef is stored.
        dictionary_name: String,
        /// The dictionary item key formatted as a string.
        dictionary_item_key: String,
    },
    /// Lookup a dictionary item via an entities named keys.
    EntityNamedKey {
        /// The entity address.
        addr: EntityAddr,
        /// The named key under which the dictionary seed URef is stored.
        dictionary_name: String,
        /// The dictionary item key formatted as a string.
        dictionary_item_key: String,
    },
    /// Lookup a dictionary item via its seed URef.
    URef {
        /// The dictionary's seed URef.
        seed_uref: URef,
        /// The dictionary item key formatted as a string.
        dictionary_item_key: String,
    },
    /// Lookup a dictionary item via its unique key.
    DictionaryItem(DictionaryAddr),
}

impl DictionaryItemIdentifier {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..5) {
            0 => DictionaryItemIdentifier::AccountNamedKey {
                hash: rng.gen(),
                dictionary_name: rng.random_string(32..64),
                dictionary_item_key: rng.random_string(32..64),
            },
            1 => DictionaryItemIdentifier::ContractNamedKey {
                hash: rng.gen(),
                dictionary_name: rng.random_string(32..64),
                dictionary_item_key: rng.random_string(32..64),
            },
            2 => DictionaryItemIdentifier::EntityNamedKey {
                addr: rng.gen(),
                dictionary_name: rng.random_string(32..64),
                dictionary_item_key: rng.random_string(32..64),
            },
            3 => DictionaryItemIdentifier::URef {
                seed_uref: rng.gen(),
                dictionary_item_key: rng.random_string(32..64),
            },
            4 => DictionaryItemIdentifier::DictionaryItem(rng.gen()),
            _ => unreachable!(),
        }
    }
}

impl ToBytes for DictionaryItemIdentifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            DictionaryItemIdentifier::AccountNamedKey {
                hash: key,
                dictionary_name,
                dictionary_item_key,
            } => {
                ACCOUNT_NAMED_KEY_TAG.write_bytes(writer)?;
                key.write_bytes(writer)?;
                dictionary_name.write_bytes(writer)?;
                dictionary_item_key.write_bytes(writer)
            }
            DictionaryItemIdentifier::ContractNamedKey {
                hash: key,
                dictionary_name,
                dictionary_item_key,
            } => {
                CONTRACT_NAMED_KEY_TAG.write_bytes(writer)?;
                key.write_bytes(writer)?;
                dictionary_name.write_bytes(writer)?;
                dictionary_item_key.write_bytes(writer)
            }
            DictionaryItemIdentifier::EntityNamedKey {
                addr,
                dictionary_name,
                dictionary_item_key,
            } => {
                ENTITY_NAMED_KEY_TAG.write_bytes(writer)?;
                addr.write_bytes(writer)?;
                dictionary_name.write_bytes(writer)?;
                dictionary_item_key.write_bytes(writer)
            }
            DictionaryItemIdentifier::URef {
                seed_uref,
                dictionary_item_key,
            } => {
                UREF_TAG.write_bytes(writer)?;
                seed_uref.write_bytes(writer)?;
                dictionary_item_key.write_bytes(writer)
            }
            DictionaryItemIdentifier::DictionaryItem(addr) => {
                DICTIONARY_ITEM_TAG.write_bytes(writer)?;
                addr.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                DictionaryItemIdentifier::AccountNamedKey {
                    hash,
                    dictionary_name,
                    dictionary_item_key,
                } => {
                    hash.serialized_length()
                        + dictionary_name.serialized_length()
                        + dictionary_item_key.serialized_length()
                }
                DictionaryItemIdentifier::ContractNamedKey {
                    hash,
                    dictionary_name,
                    dictionary_item_key,
                } => {
                    hash.serialized_length()
                        + dictionary_name.serialized_length()
                        + dictionary_item_key.serialized_length()
                }
                DictionaryItemIdentifier::EntityNamedKey {
                    addr,
                    dictionary_name,
                    dictionary_item_key,
                } => {
                    addr.serialized_length()
                        + dictionary_name.serialized_length()
                        + dictionary_item_key.serialized_length()
                }
                DictionaryItemIdentifier::URef {
                    seed_uref,
                    dictionary_item_key,
                } => seed_uref.serialized_length() + dictionary_item_key.serialized_length(),
                DictionaryItemIdentifier::DictionaryItem(addr) => addr.serialized_length(),
            }
    }
}

impl FromBytes for DictionaryItemIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ACCOUNT_NAMED_KEY_TAG => {
                let (key, remainder) = FromBytes::from_bytes(remainder)?;
                let (dictionary_name, remainder) = String::from_bytes(remainder)?;
                let (dictionary_item_key, remainder) = String::from_bytes(remainder)?;
                Ok((
                    DictionaryItemIdentifier::AccountNamedKey {
                        hash: key,
                        dictionary_name,
                        dictionary_item_key,
                    },
                    remainder,
                ))
            }
            CONTRACT_NAMED_KEY_TAG => {
                let (key, remainder) = FromBytes::from_bytes(remainder)?;
                let (dictionary_name, remainder) = String::from_bytes(remainder)?;
                let (dictionary_item_key, remainder) = String::from_bytes(remainder)?;
                Ok((
                    DictionaryItemIdentifier::ContractNamedKey {
                        hash: key,
                        dictionary_name,
                        dictionary_item_key,
                    },
                    remainder,
                ))
            }
            ENTITY_NAMED_KEY_TAG => {
                let (addr, remainder) = FromBytes::from_bytes(remainder)?;
                let (dictionary_name, remainder) = String::from_bytes(remainder)?;
                let (dictionary_item_key, remainder) = String::from_bytes(remainder)?;
                Ok((
                    DictionaryItemIdentifier::EntityNamedKey {
                        addr,
                        dictionary_name,
                        dictionary_item_key,
                    },
                    remainder,
                ))
            }
            UREF_TAG => {
                let (seed_uref, remainder) = FromBytes::from_bytes(remainder)?;
                let (dictionary_item_key, remainder) = String::from_bytes(remainder)?;
                Ok((
                    DictionaryItemIdentifier::URef {
                        seed_uref,
                        dictionary_item_key,
                    },
                    remainder,
                ))
            }
            DICTIONARY_ITEM_TAG => {
                let (addr, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((DictionaryItemIdentifier::DictionaryItem(addr), remainder))
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

        let val = DictionaryItemIdentifier::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
