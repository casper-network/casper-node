use casper_types::{Block, BlockSignatures};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A wrapper around `Block` for the purposes of fetching blocks by height in linear chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockWithMetadata {
    pub block: Block,
    pub block_signatures: BlockSignatures,
}

impl fmt::Display for BlockWithMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "block #{}, {}, with {} block signatures",
            self.block.height(),
            self.block.hash(),
            self.block_signatures.len()
        )
    }
}
