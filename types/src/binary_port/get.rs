//! The Get variant of the request to the binary port.

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, Key, KeyTag,
};
use alloc::{string::String, vec::Vec};

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

use super::{db_id::DbId, non_persistent_data_request::NonPersistedDataRequest};

const DB_TAG: u8 = 0;
const NON_PERSISTED_DATA_TAG: u8 = 1;
const STATE_TAG: u8 = 2;
const ALL_VALUES_TAG: u8 = 3;
const TRIE_TAG: u8 = 4;

/// The kind of the `Get` operation.
#[derive(Clone, Debug, PartialEq)]
pub enum GetRequest {
    /// Gets data stored under the given key from the given db.
    Db {
        /// Id of the database.
        db: DbId,
        /// Key.
        key: Vec<u8>,
    },
    /// Gets a data which is not persisted, and build on demand by the `casper-node`.
    NonPersistedData(NonPersistedDataRequest),
    /// Gets data from the global state.
    State {
        /// State root hash
        state_root_hash: Digest,
        /// Key under which data is stored.
        base_key: Key,
        /// Path under which the value is stored.
        path: Vec<String>,
    },
    /// Get all values under the given key tag.
    AllValues {
        /// State root hash
        state_root_hash: Digest,
        /// Key tag
        key_tag: KeyTag,
    },
    /// Gets value from the trie.
    Trie {
        /// A trie key.
        trie_key: Digest,
    },
}

impl GetRequest {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..5) {
            0 => GetRequest::Db {
                db: DbId::random(rng),
                key: rng.random_vec(16..32),
            },
            1 => GetRequest::NonPersistedData(NonPersistedDataRequest::random(rng)),
            2 => {
                let path_count = rng.gen_range(10..20);
                GetRequest::State {
                    state_root_hash: Digest::random(rng),
                    base_key: rng.gen(),
                    path: std::iter::repeat_with(|| rng.random_string(32..64))
                        .take(path_count)
                        .collect(),
                }
            }
            3 => GetRequest::AllValues {
                state_root_hash: Digest::random(rng),
                key_tag: KeyTag::random(rng),
            },
            4 => GetRequest::Trie {
                trie_key: Digest::random(rng),
            },
            _ => panic!(),
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
            GetRequest::Db { db, key } => {
                DB_TAG.write_bytes(writer)?;
                db.write_bytes(writer)?;
                key.write_bytes(writer)
            }
            GetRequest::NonPersistedData(inner) => {
                NON_PERSISTED_DATA_TAG.write_bytes(writer)?;
                inner.write_bytes(writer)
            }
            GetRequest::State {
                state_root_hash,
                base_key,
                path,
            } => {
                STATE_TAG.write_bytes(writer)?;
                state_root_hash.write_bytes(writer)?;
                base_key.write_bytes(writer)?;
                path.write_bytes(writer)
            }
            GetRequest::AllValues {
                state_root_hash,
                key_tag,
            } => {
                ALL_VALUES_TAG.write_bytes(writer)?;
                state_root_hash.write_bytes(writer)?;
                (*key_tag as u8).write_bytes(writer)
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
                GetRequest::Db { db, key } => db.serialized_length() + key.serialized_length(),
                GetRequest::NonPersistedData(inner) => inner.serialized_length(),
                GetRequest::State {
                    state_root_hash,
                    base_key,
                    path,
                } => {
                    state_root_hash.serialized_length()
                        + base_key.serialized_length()
                        + path.serialized_length()
                }
                GetRequest::AllValues {
                    state_root_hash,
                    key_tag,
                } => state_root_hash.serialized_length() + (*key_tag as u8).serialized_length(),
                GetRequest::Trie { trie_key } => trie_key.serialized_length(),
            }
    }
}

impl FromBytes for GetRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = FromBytes::from_bytes(bytes)?;
        match tag {
            DB_TAG => {
                let (db, remainder) = FromBytes::from_bytes(remainder)?;
                let (key, remainder) = Bytes::from_bytes(remainder)?;
                Ok((
                    GetRequest::Db {
                        db,
                        key: key.into(),
                    },
                    remainder,
                ))
            }
            NON_PERSISTED_DATA_TAG => {
                let (non_persisted_data, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((GetRequest::NonPersistedData(non_persisted_data), remainder))
            }
            STATE_TAG => {
                let (state_root_hash, remainder) = FromBytes::from_bytes(remainder)?;
                let (base_key, remainder) = FromBytes::from_bytes(remainder)?;
                let (path, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    GetRequest::State {
                        state_root_hash,
                        base_key,
                        path,
                    },
                    remainder,
                ))
            }
            ALL_VALUES_TAG => {
                let (state_root_hash, remainder) = FromBytes::from_bytes(remainder)?;
                let (key_tag, remainder) = u8::from_bytes(remainder)?;
                let key_tag = match key_tag {
                    0 => KeyTag::Account,
                    1 => KeyTag::Hash,
                    2 => KeyTag::URef,
                    3 => KeyTag::Transfer,
                    4 => KeyTag::DeployInfo,
                    5 => KeyTag::EraInfo,
                    6 => KeyTag::Balance,
                    7 => KeyTag::Bid,
                    8 => KeyTag::Withdraw,
                    9 => KeyTag::Dictionary,
                    10 => KeyTag::SystemContractRegistry,
                    11 => KeyTag::EraSummary,
                    12 => KeyTag::Unbond,
                    13 => KeyTag::ChainspecRegistry,
                    14 => KeyTag::ChecksumRegistry,
                    15 => KeyTag::BidAddr,
                    16 => KeyTag::Package,
                    17 => KeyTag::AddressableEntity,
                    18 => KeyTag::ByteCode,
                    19 => KeyTag::Message,
                    _ => return Err(bytesrepr::Error::Formatting),
                };
                Ok((
                    GetRequest::AllValues {
                        state_root_hash,
                        key_tag,
                    },
                    remainder,
                ))
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
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = GetRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
