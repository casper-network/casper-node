#[cfg(feature = "datasize")]
use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::BlockBody;
use crate::{DeployHash, PublicKey};

/// A JSON-friendly representation of [`BlockBody`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[schemars(description = "The body portion of a block.")]
#[serde(deny_unknown_fields)]
pub struct JsonBlockBody {
    /// The public key of the validator which proposed the block.
    pub proposer: PublicKey,
    /// The deploy hashes of the non-transfer deploys within the block.
    pub deploy_hashes: Vec<DeployHash>,
    /// The deploy hashes of the transfers within the block.
    pub transfer_hashes: Vec<DeployHash>,
}

impl From<BlockBody> for JsonBlockBody {
    fn from(body: BlockBody) -> Self {
        match body {
            BlockBody::V1(v1) => JsonBlockBody {
                proposer: v1.proposer().clone(),
                deploy_hashes: v1.deploy_hashes().into(),
                transfer_hashes: v1.transfer_hashes().into(),
            },
            BlockBody::V2(v2) => JsonBlockBody {
                proposer: v2.proposer().clone(),
                deploy_hashes: v2.deploy_hashes().into(),
                transfer_hashes: v2.transfer_hashes().into(),
            },
        }
    }
}
