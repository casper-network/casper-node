use std::{
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use derive_more::Display;
use serde::{de::DeserializeOwned, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::effect::GossipTarget;

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
)]
#[repr(u8)]
pub enum Tag {
    /// A deploy identified by its hash and its approvals hash.
    #[display(fmt = "deploy")]
    Deploy,
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

/// A trait unifying the common pieces of the `FetcherItem` and `GossiperItem` traits.
pub(crate) trait Item:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display + Eq
{
    /// The type of ID of the item.
    type Id: Clone + Eq + Hash + Serialize + DeserializeOwned + Send + Sync + Debug + Display;

    /// The ID of the specific item.
    fn id(&self) -> Self::Id;
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Debug, DataSize)]
pub(crate) struct EmptyValidationMetadata;

impl Display for EmptyValidationMetadata {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(formatter, "no validation metadata")
    }
}

/// A trait which allows an implementing type to be used by a fetcher component.
pub(crate) trait FetcherItem: Item {
    /// The error type returned when validating to get the ID of the item.
    type ValidationError: std::error::Error + Debug + Display;
    type ValidationMetadata: Eq + Clone + Serialize + Debug + DataSize + Send;

    /// The tag representing the type of the item.
    const TAG: Tag;

    /// Checks cryptographic validity of the item, and returns an error if invalid.
    fn validate(&self, metadata: &Self::ValidationMetadata) -> Result<(), Self::ValidationError>;
}

/// A trait which allows an implementing type to be used by a gossiper component.
pub(crate) trait GossiperItem: Item {
    /// Whether the item's ID _is_ the complete item or not.
    const ID_IS_COMPLETE_ITEM: bool;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool;

    /// Returns the era ID of the item, if one is relevant to it, e.g. blocks, finality signatures.
    fn target(&self) -> GossipTarget;
}
