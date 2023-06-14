#[cfg(feature = "datasize")]
use datasize::DataSize;
use once_cell::sync::OnceCell;
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
            proposer: body.proposer,
            deploy_hashes: body.deploy_hashes,
            transfer_hashes: body.transfer_hashes,
        }
    }
}

impl From<JsonBlockBody> for BlockBody {
    fn from(json_body: JsonBlockBody) -> Self {
        BlockBody {
            proposer: json_body.proposer,
            deploy_hashes: json_body.deploy_hashes,
            transfer_hashes: json_body.transfer_hashes,
            hash: OnceCell::new(),
        }
    }
}
