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
        JsonBlockBody {
            proposer: body.proposer().clone(),
            deploy_hashes: body.deploy_hashes().into(),
            transfer_hashes: body.transfer_hashes().into(),
        }
    }
}
