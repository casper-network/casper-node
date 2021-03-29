use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use derive_more::Display;
use serde::{de::DeserializeOwned, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::types::{BlockHash, BlockHeader, BlockHeaderAndMetadata};
use casper_execution_engine::{
    shared::{newtypes::Blake2bHash, stored_value::StoredValue},
    storage::trie::Trie,
};
use casper_types::{bytesrepr::ToBytes, Key};

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
    /// A deploy.
    Deploy,
    /// A block.
    Block,
    /// A gossiped public listening address.
    GossipedAddress,
    /// A block requested by its height in the linear chain.
    BlockByHeight,
    /// A block header requested by its hash.
    BlockHeaderByHash,
    /// A block header and its finality signatures requested by its height in the linear chain.
    BlockHeaderAndFinalitySignaturesByHeight,
    /// A trie from the global trie merkle tree in the execution engine.
    Trie,
}

/// A trait which allows an implementing type to be used by the gossiper and fetcher components, and
/// furthermore allows generic network messages to include this type due to the provision of the
/// type-identifying `TAG`.
pub trait Item: Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display {
    /// The type of ID of the item.
    type Id: Copy + Eq + Hash + Serialize + DeserializeOwned + Send + Sync + Debug + Display;
    /// The tag representing the type of the item.
    const TAG: Tag;
    /// Whether the item's ID _is_ the complete item or not.
    const ID_IS_COMPLETE_ITEM: bool;

    /// The ID of the specific item.
    fn id(&self) -> Self::Id;
}

impl Item for Trie<Key, StoredValue> {
    type Id = Blake2bHash;
    const TAG: Tag = Tag::Trie;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn id(&self) -> Self::Id {
        let node_bytes = self.to_bytes().expect("Could not serialize trie to bytes");
        Blake2bHash::new(&node_bytes)
    }
}

impl Item for BlockHeader {
    type Id = BlockHash;
    const TAG: Tag = Tag::BlockHeaderByHash;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn id(&self) -> Self::Id {
        self.hash()
    }
}

impl Item for BlockHeaderAndMetadata {
    type Id = u64;
    const TAG: Tag = Tag::BlockHeaderAndFinalitySignaturesByHeight;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn id(&self) -> Self::Id {
        self.block_header.height()
    }
}
