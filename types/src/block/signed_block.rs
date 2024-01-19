use core::fmt::{self, Display, Formatter};

use crate::{Block, BlockSignatures};
#[cfg(any(feature = "std", feature = "json-schema", test))]
use serde::{Deserialize, Serialize};

/// A block and signatures for that block.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    any(feature = "std", feature = "json-schema", test),
    derive(Serialize, Deserialize)
)]
pub struct SignedBlock {
    /// Block.
    pub(crate) block: Block,
    // The signatures of the block.
    pub(crate) block_signatures: BlockSignatures,
}

impl SignedBlock {
    /// Creates a new `SignedBlock`.
    pub fn new(block: Block, block_signatures: BlockSignatures) -> Self {
        Self {
            block,
            block_signatures,
        }
    }

    /// Returns the inner block.
    pub fn block(&self) -> &Block {
        &self.block
    }

    /// Converts `self` into the block and signatures.
    pub fn into_inner(self) -> (Block, BlockSignatures) {
        (self.block, self.block_signatures)
    }
}

impl Display for SignedBlock {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "block #{}, {}, with {} block signatures",
            self.block.height(),
            self.block.hash(),
            self.block_signatures.len()
        )
    }
}
