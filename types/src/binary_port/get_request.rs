//! The get request

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, Key,
};

use super::{db_id::DbId, NonPersistedDataRequest};

const DB_TAG: u8 = 0;
const NON_PERSISTED_DATA_TAG: u8 = 1;
const STATE_TAG: u8 = 2;

/// The kind of the `Get` operation.
#[derive(Debug)]
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
            }
    }
}

impl FromBytes for GetRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            DB_TAG => {
                let (db, remainder) = DbId::from_bytes(remainder)?;
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
                let (non_persisted_data, remainder) =
                    NonPersistedDataRequest::from_bytes(remainder)?;
                Ok((GetRequest::NonPersistedData(non_persisted_data), remainder))
            }
            STATE_TAG => {
                let (state_root_hash, remainder) = Digest::from_bytes(remainder)?;
                let (base_key, remainder) = Key::from_bytes(remainder)?;
                let (path, remainder) = Vec::<String>::from_bytes(remainder)?;
                Ok((
                    GetRequest::State {
                        state_root_hash,
                        base_key,
                        path,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
