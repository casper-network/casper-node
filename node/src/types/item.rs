use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use derive_more::Display;
use serde::{de::DeserializeOwned, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

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
}

/// A trait which allows an implementing type to be used by the gossiper and fetcher components, and
/// furthermore allows generic network messages to include this type due to the provision of the
/// type-identifying `TAG`.
pub trait Item: Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display {
    /// The type of ID of the item.
    type Id: Copy + Eq + Hash + Debug + Display + Serialize + DeserializeOwned + Send + Sync;
    /// The tag representing the type of the item.
    const TAG: Tag;

    /// The ID of the specific item.
    fn id(&self) -> Self::Id;
}
