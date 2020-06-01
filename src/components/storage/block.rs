use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use serde::{de::DeserializeOwned, Serialize};

pub(crate) trait BlockType:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display
{
    type Hash: Copy
        + Clone
        + Ord
        + PartialOrd
        + Eq
        + PartialEq
        + Hash
        + Debug
        + Display
        + Send
        + Sync;

    fn hash(&self) -> &Self::Hash;
}
