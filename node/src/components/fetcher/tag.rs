use std::hash::Hash;

use datasize::DataSize;
use derive_more::Display;
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::EnumIter;

/// An identifier for a specific type implementing the `Item` trait.  Each different implementing
/// type should have a unique `Tag` variant.
#[derive(
    Clone,
    Copy,
    DataSize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize_repr,
    Deserialize_repr,
    Debug,
    Display,
    EnumIter,
)]
#[repr(u8)]
pub enum Tag {
    /// A transaction identified by its hash and its approvals hash.
    #[display(fmt = "transaction")]
    Transaction,
    /// A legacy deploy identified by its hash alone.
    #[display(fmt = "legacy deploy")]
    LegacyDeploy,
    /// A block.
    #[display(fmt = "block")]
    Block,
    /// A block header.
    #[display(fmt = "block header")]
    BlockHeader,
    /// A trie or chunk of a trie from global state.
    #[display(fmt = "trie or chunk")]
    TrieOrChunk,
    /// A finality signature for a block.
    #[display(fmt = "finality signature")]
    FinalitySignature,
    /// Headers and signatures required to prove that if a given trusted block hash is on the
    /// correct chain, then so is a later header, which should be the most recent one according
    /// to the sender.
    #[display(fmt = "sync leap")]
    SyncLeap,
    /// The hashes of the finalized deploy approvals sets for a single block.
    #[display(fmt = "approvals hashes")]
    ApprovalsHashes,
    /// The execution results for a single block.
    #[display(fmt = "block execution results")]
    BlockExecutionResults,
}
