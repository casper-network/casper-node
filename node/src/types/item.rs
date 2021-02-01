use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use derive_more::Display;
use serde::{de::DeserializeOwned, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

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
    Deploy = 0,
    /// A block.
    Block = 1,
    /// A block requested by its height in the linear chain.
    // TODO: This is strange tag to have, it does not specify a value, but selects a function
    // instead. This should be cleaned up.
    BlockByHeight = 3,
}

/// A trait which allows an implementing type to be used by the gossiper and fetcher components, and
/// furthermore allows generic network messages to include this type due to the provision of the
/// type-identifying `TAG`.
pub trait Item: Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display {
    /// The type of ID of the item.
    type Id: Copy + Eq + Hash + Serialize + DeserializeOwned + Send + Sync + Debug + Display;

    /// The tag representing the type of the item.
    const TAG: Tag;

    /// The ID of the specific item.
    fn id(&self) -> Self::Id;
}

impl Item for Trie<Key, StoredValue> {
    type Id = Blake2bHash;
    const TAG: Tag = Tag::Deploy;

    fn id(&self) -> Self::Id {
        let node_bytes = self.to_bytes().expect("Could not serialize trie to bytes");
        Blake2bHash::new(&node_bytes)
    }
}
