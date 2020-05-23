use std::{fmt::Debug, hash::Hash};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub(crate) trait BlockType: Clone + Serialize + DeserializeOwned + Send + Debug {
    type Name: Copy + Hash + PartialOrd + Ord + PartialEq + Eq + Debug + Send;

    fn name(&self) -> &Self::Name;
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub(crate) struct CLBlock {
    name: u8,
    etc: u64,
}

impl CLBlock {
    pub(crate) fn new(name: u8, etc: u64) -> Self {
        CLBlock { name, etc }
    }
}

impl BlockType for CLBlock {
    type Name = u8;

    fn name(&self) -> &Self::Name {
        &self.name
    }
}
