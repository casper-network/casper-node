//! Support for global state queries.
use casper_storage::global_state::trie::merkle_proof::TrieMerkleProof;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, Key, StoredValue,
};

use crate::tracking_copy::TrackingCopyQueryResult;

/// Result of a global state query request.
#[derive(Debug)]
pub enum QueryResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// Circular reference error.
    CircularReference(String),
    /// Depth limit reached.
    DepthLimit {
        /// Current depth limit.
        depth: u64,
    },
    /// Successful query.
    Success {
        /// Stored value under a path.
        value: Box<StoredValue>,
        /// Merkle proof of the query.
        proofs: Vec<TrieMerkleProof<Key, StoredValue>>,
    },
}

/// Request for a global state query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRequest {
    state_hash: Digest,
    key: Key,
    path: Vec<String>,
}

impl QueryRequest {
    /// Creates new request object.
    pub fn new(state_hash: Digest, key: Key, path: Vec<String>) -> Self {
        QueryRequest {
            state_hash,
            key,
            path,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns a key.
    pub fn key(&self) -> Key {
        self.key
    }

    /// Returns a query path.
    pub fn path(&self) -> &[String] {
        &self.path
    }
}

impl From<TrackingCopyQueryResult> for QueryResult {
    fn from(tracking_copy_query_result: TrackingCopyQueryResult) -> Self {
        match tracking_copy_query_result {
            TrackingCopyQueryResult::ValueNotFound(message) => QueryResult::ValueNotFound(message),
            TrackingCopyQueryResult::CircularReference(message) => {
                QueryResult::CircularReference(message)
            }
            TrackingCopyQueryResult::Success { value, proofs } => {
                let value = Box::new(value);
                QueryResult::Success { value, proofs }
            }
            TrackingCopyQueryResult::DepthLimit { depth } => QueryResult::DepthLimit { depth },
        }
    }
}

const ROOT_NOT_FOUND_TAG: u8 = 0;
const VALUE_NOT_FOUND_TAG: u8 = 1;
const CIRCULAR_REFERENCE_TAG: u8 = 2;
const DEPTH_LIMIT_TAG: u8 = 3;
const SUCCESS_TAG: u8 = 4;

impl ToBytes for QueryResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            QueryResult::RootNotFound => ROOT_NOT_FOUND_TAG.write_bytes(writer),
            QueryResult::ValueNotFound(err) => {
                VALUE_NOT_FOUND_TAG.write_bytes(writer)?;
                err.write_bytes(writer)
            }
            QueryResult::CircularReference(err) => {
                CIRCULAR_REFERENCE_TAG.write_bytes(writer)?;
                err.write_bytes(writer)
            }
            QueryResult::DepthLimit { depth } => {
                DEPTH_LIMIT_TAG.write_bytes(writer)?;
                depth.write_bytes(writer)
            }
            QueryResult::Success { value, proofs } => {
                SUCCESS_TAG.write_bytes(writer)?;
                value.write_bytes(writer)?;
                proofs.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                QueryResult::RootNotFound => 0,
                QueryResult::ValueNotFound(err) => err.serialized_length(),
                QueryResult::CircularReference(err) => err.serialized_length(),
                QueryResult::DepthLimit { depth } => depth.serialized_length(),
                QueryResult::Success { value, proofs } => {
                    value.serialized_length() + proofs.serialized_length()
                }
            }
    }
}

impl FromBytes for QueryResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ROOT_NOT_FOUND_TAG => Ok((QueryResult::RootNotFound, remainder)),
            VALUE_NOT_FOUND_TAG => {
                let (err, remainder) = String::from_bytes(remainder)?;
                Ok((QueryResult::ValueNotFound(err), remainder))
            }
            CIRCULAR_REFERENCE_TAG => {
                let (err, remainder) = String::from_bytes(remainder)?;
                Ok((QueryResult::CircularReference(err), remainder))
            }
            DEPTH_LIMIT_TAG => {
                let (depth, remainder) = u64::from_bytes(remainder)?;
                Ok((QueryResult::DepthLimit { depth }, remainder))
            }
            SUCCESS_TAG => {
                let (value, remainder) = StoredValue::from_bytes(remainder)?;
                let (proofs, remainder) =
                    Vec::<TrieMerkleProof<Key, StoredValue>>::from_bytes(remainder)?;
                Ok((
                    QueryResult::Success {
                        value: Box::new(value),
                        proofs,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
