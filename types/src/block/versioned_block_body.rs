use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{BlockBody, BlockBodyV1, DeployHash};

use super::block_body::BlockBodyV2;

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
