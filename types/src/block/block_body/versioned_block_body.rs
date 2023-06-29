use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockBody, DeployHash,
};

use super::{block_body_v1::BlockBodyV1, block_body_v2::BlockBodyV2};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for block body v1.
pub const BLOCK_BODY_V1_TAG: u8 = 0;
/// Tag for block body v2.
pub const BLOCK_BODY_V2_TAG: u8 = 1;

/// The versioned body portion of a block. It encapsulates different variants of the BlockBody
/// struct.
// TODO[RC]: Moving to a separate module after merged with Fraser's types rework
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[derive(Clone, Eq, Serialize, Deserialize, Debug)]
pub enum VersionedBlockBody {
    /// The legacy, initial version of the body portion of a block.
    V1(BlockBodyV1),
    /// The version 2 of the body portion of a block, which includes the
    /// `past_finality_signatures`.
    V2(BlockBodyV2),
}

impl VersionedBlockBody {
    /// Retrieves the deploy hashes within the block.
    pub fn deploy_hashes(&self) -> &Vec<DeployHash> {
        match self {
            VersionedBlockBody::V1(v1) => &v1.deploy_hashes,
            VersionedBlockBody::V2(v2) => &v2.deploy_hashes,
        }
    }

    /// Retrieves the transfer hashes within the block.
    pub fn transfer_hashes(&self) -> &Vec<DeployHash> {
        match self {
            VersionedBlockBody::V1(v1) => &v1.transfer_hashes,
            VersionedBlockBody::V2(v2) => &v2.transfer_hashes,
        }
    }

    /// Returns deploy hashes of transactions in an order in which they were executed.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes()
            .iter()
            .chain(self.transfer_hashes().iter())
    }
}

// TODO[RC]: We probably don't need `Eq` on `VersionedBlockBody` - remove this and make sure that
// only correct version of the BlockBody are allowed to be compared.
impl PartialEq for VersionedBlockBody {
    fn eq(&self, other: &VersionedBlockBody) -> bool {
        match (self, other) {
            (VersionedBlockBody::V1(lhs), VersionedBlockBody::V1(rhs)) => lhs.eq(rhs),
            (VersionedBlockBody::V2(lhs), VersionedBlockBody::V2(rhs)) => lhs.eq(rhs),
            _ => {
                panic!("BlockBody structs of different versions should not be compared")
            }
        }
    }
}

impl Display for VersionedBlockBody {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            VersionedBlockBody::V1(v1) => Display::fmt(&v1, formatter),
            VersionedBlockBody::V2(v2) => Display::fmt(&v2, formatter),
        }
    }
}

impl From<&VersionedBlockBody> for BlockBody {
    fn from(value: &VersionedBlockBody) -> Self {
        match value {
            VersionedBlockBody::V1(_) => todo!(),
            VersionedBlockBody::V2(v2) => v2.clone(),
        }
    }
}

impl ToBytes for VersionedBlockBody {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            VersionedBlockBody::V1(v1) => {
                buffer.insert(0, BLOCK_BODY_V1_TAG);
                buffer.extend(v1.to_bytes()?);
            }
            VersionedBlockBody::V2(v2) => {
                buffer.insert(0, BLOCK_BODY_V2_TAG);
                buffer.extend(v2.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                VersionedBlockBody::V1(v1) => v1.serialized_length(),
                VersionedBlockBody::V2(v2) => v2.serialized_length(),
            }
    }
}

impl FromBytes for VersionedBlockBody {
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
        block::{block_v1::BlockV1, block_v2::BlockV2},
        bytesrepr,
        testing::TestRng,
    };

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let block_body_v1 = BlockV1::random(rng).body().clone();
        let versioned_block_body = VersionedBlockBody::V1(block_body_v1);
        bytesrepr::test_serialization_roundtrip(&versioned_block_body);

        let block_body_v2 = BlockV2::random(rng).body().clone();
        let versioned_block_body = VersionedBlockBody::V2(block_body_v2);
        bytesrepr::test_serialization_roundtrip(&versioned_block_body);
    }
}
