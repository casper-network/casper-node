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
    Deploy,
    /// A legacy deploy identified by its hash alone.
    LegacyDeploy,
    /// A block.
    Block,
    /// A gossiped public listening address.
    GossipedAddress,
    /// A block header requested by its hash.
    BlockHeaderByHash,
    /// A trie or chunk from the global Merkle tree in the execution engine.
    TrieOrChunk,
    /// A full block and its deploys.
    BlockAndDeploysByHash,
    /// A batch of block headers requested by their lower and upper height indices.
    BlockHeaderBatch,
    /// A single block signature for a block.
    FinalitySignature,
    /// Finality signatures for a block requested by the block's hash.
    FinalitySignaturesByHash,
    /// Headers and signatures required to prove that if a given trusted block hash is on the
    /// correct chain, then so is a later header, which should be the most recent one according
    /// to the sender.
    SyncLeap,
    /// The hashes of the finalized deploy approvals sets.
    ApprovalsHashes,
    /// Block execution results fetched during syncing.
    BlockExecutionResults,
}

/// A trait unifying the common pieces of the `FetcherItem` and `GossiperItem` traits.
pub(crate) trait Item:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display + Eq
{
    /// The type of ID of the item.
    type Id: Clone + Eq + Hash + Serialize + DeserializeOwned + Send + Sync + Debug + Display;

    /// The tag representing the type of the item.
    const TAG: Tag;

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
    type ValidationMetadata: Eq + Clone + Serialize + Debug + Display + DataSize + Send;

    /// Checks cryptographic validity of the item, and returns an error if invalid.
    fn validate(&self, metadata: &Self::ValidationMetadata) -> Result<(), Self::ValidationError>;
}

/// A trait which allows an implementing type to be used by a gossiper component.
pub(crate) trait GossiperItem: Item {
    /// Whether the item's ID _is_ the complete item or not.
    const ID_IS_COMPLETE_ITEM: bool;

    /// Returns the era ID of the item, if one is relevant to it, e.g. blocks, finality signatures.
    fn target(&self) -> GossipTarget;
}
