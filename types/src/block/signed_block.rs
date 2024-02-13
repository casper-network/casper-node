use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Block, BlockSignatures,
};
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

impl FromBytes for SignedBlock {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block, bytes) = FromBytes::from_bytes(bytes)?;
        let (block_signatures, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((SignedBlock::new(block, block_signatures), bytes))
    }
}

impl ToBytes for SignedBlock {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let mut buf = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buf)?;
        Ok(buf)
    }

    fn write_bytes(&self, bytes: &mut Vec<u8>) -> Result<(), crate::bytesrepr::Error> {
        self.block.write_bytes(bytes)?;
        self.block_signatures.write_bytes(bytes)?;
        Ok(())
    }

    fn serialized_length(&self) -> usize {
        self.block.serialized_length() + self.block_signatures.serialized_length()
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
