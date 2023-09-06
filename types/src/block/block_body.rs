mod block_body_v1;
mod block_body_v2;

pub use block_body_v1::BlockBodyV1;
pub use block_body_v2::BlockBodyV2;

use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    DeployHash,
};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for block body v1.
pub const BLOCK_BODY_V1_TAG: u8 = 0;
/// Tag for block body v2.
pub const BLOCK_BODY_V2_TAG: u8 = 1;

/// The versioned body portion of a block. It encapsulates different variants of the BlockBody
/// struct.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(any(feature = "testing", test), derive(PartialEq))]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum BlockBody {
    /// The legacy, initial version of the body portion of a block.
    V1(BlockBodyV1),
    /// The version 2 of the body portion of a block, which includes the
    /// `past_finality_signatures`.
    V2(BlockBodyV2),
}

impl BlockBody {
    /// Retrieves the deploy hashes within the block.
    pub fn deploy_hashes(&self) -> &Vec<DeployHash> {
        match self {
            BlockBody::V1(v1) => &v1.deploy_hashes,
            BlockBody::V2(v2) => &v2.deploy_hashes,
        }
    }

    /// Retrieves the transfer hashes within the block.
    pub fn transfer_hashes(&self) -> &Vec<DeployHash> {
        match self {
            BlockBody::V1(v1) => &v1.transfer_hashes,
            BlockBody::V2(v2) => &v2.transfer_hashes,
        }
    }

    /// Returns the deploy and transfer hashes in the order in which they were executed.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> + Clone {
        self.deploy_hashes()
            .iter()
            .chain(self.transfer_hashes().iter())
    }
}

impl Display for BlockBody {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            BlockBody::V1(v1) => Display::fmt(&v1, formatter),
            BlockBody::V2(v2) => Display::fmt(&v2, formatter),
        }
    }
}

impl From<BlockBodyV1> for BlockBody {
    fn from(body: BlockBodyV1) -> Self {
        BlockBody::V1(body)
    }
}

impl From<&BlockBodyV2> for BlockBody {
    fn from(body: &BlockBodyV2) -> Self {
        BlockBody::V2(body.clone())
    }
}

impl ToBytes for BlockBody {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            BlockBody::V1(v1) => {
                buffer.insert(0, BLOCK_BODY_V1_TAG);
                buffer.extend(v1.to_bytes()?);
            }
            BlockBody::V2(v2) => {
                buffer.insert(0, BLOCK_BODY_V2_TAG);
                buffer.extend(v2.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                BlockBody::V1(v1) => v1.serialized_length(),
                BlockBody::V2(v2) => v2.serialized_length(),
            }
    }
}

impl FromBytes for BlockBody {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            BLOCK_BODY_V1_TAG => {
                let (body, remainder): (BlockBodyV1, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::V1(body), remainder))
            }
            BLOCK_BODY_V2_TAG => {
                let (body, remainder): (BlockBodyV2, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::V2(body), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        block::{
            block_v1::BlockV1,
            test_block_builder::{FromTestBlockBuilder, TestBlockBuilder},
        },
        bytesrepr,
        testing::TestRng,
    };

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let block_body_v1 = BlockV1::build_for_test(TestBlockBuilder::new(), rng)
            .body()
            .clone();
        let block_body = BlockBody::V1(block_body_v1);
        bytesrepr::test_serialization_roundtrip(&block_body);

        let block_body_v2 = TestBlockBuilder::new().build(rng).body().clone();
        let block_body = BlockBody::V2(block_body_v2);
        bytesrepr::test_serialization_roundtrip(&block_body);
    }
}
