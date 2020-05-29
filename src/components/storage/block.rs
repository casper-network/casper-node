use std::{
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub(crate) trait BlockType:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display
{
    type Name: Copy + Hash + PartialOrd + Ord + PartialEq + Eq + Debug + Display + Send + Sync;

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

impl Display for CLBlock {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "Block({})", self.name)
    }
}
