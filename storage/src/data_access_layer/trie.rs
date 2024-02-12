use casper_types::Digest;

use crate::global_state::{error::Error as GlobalStateError, trie::TrieRaw};

/// Request for a trie element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrieRequest {
    trie_key: Digest,
    chunk_id: Option<u64>,
}

impl TrieRequest {
    /// Creates an instance of TrieRequest.
    pub fn new(trie_key: Digest, chunk_id: Option<u64>) -> Self {
        TrieRequest { trie_key, chunk_id }
    }

    /// Trie key.
    pub fn trie_key(&self) -> Digest {
        self.trie_key
    }

    /// Chunk id.
    pub fn chunk_id(&self) -> Option<u64> {
        self.chunk_id
    }

    /// Has chunk id.
    pub fn has_chunk_id(&self) -> bool {
        self.chunk_id.is_some()
    }
}

/// A trie element.
#[derive(Debug)]
pub enum TrieElement {
    /// Raw bytes.
    Raw(TrieRaw),
    /// Chunk.
    Chunked(TrieRaw, u64),
}

/// Represents a result of a `trie` request.
#[derive(Debug)]
pub enum TrieResult {
    /// Value not found.
    ValueNotFound(String),
    /// The trie element at the specified key.
    Success {
        /// A trie element.
        element: TrieElement,
    },
    /// Failed to get the trie element.
    Failure(GlobalStateError),
}

impl TrieResult {
    pub fn into_legacy(self) -> Result<Option<TrieRaw>, GlobalStateError> {
        match self {
            TrieResult::ValueNotFound(_) => Ok(None),
            TrieResult::Success { element } => match element {
                TrieElement::Raw(raw) | TrieElement::Chunked(raw, _) => Ok(Some(raw)),
            },
            TrieResult::Failure(err) => Err(err),
        }
    }
}

/// Request for a trie element to be persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutTrieRequest {
    raw: TrieRaw,
}

impl PutTrieRequest {
    /// Creates an instance of PutTrieRequest.
    pub fn new(raw: TrieRaw) -> Self {
        PutTrieRequest { raw }
    }

    /// The raw bytes of the trie element.
    pub fn raw(&self) -> &TrieRaw {
        &self.raw
    }

    pub fn take_raw(self) -> TrieRaw {
        self.raw
    }
}

/// Represents a result of a `put_trie` request.
#[derive(Debug)]
pub enum PutTrieResult {
    /// The trie element is persisted.
    Success {
        /// The hash of the persisted trie element.
        hash: Digest,
    },
    /// Failed to persist the trie element.
    Failure(GlobalStateError),
}

impl PutTrieResult {
    /// Returns a Result matching the original api for this functionality.
    pub fn as_legacy(&self) -> Result<Digest, GlobalStateError> {
        match self {
            PutTrieResult::Success { hash } => Ok(*hash),
            PutTrieResult::Failure(err) => Err(err.clone()),
        }
    }
}
