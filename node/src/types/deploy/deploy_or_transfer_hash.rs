use datasize::DataSize;
use derive_more::Display;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::DeployHash;

/// The [`DeployHash`] stored in a way distinguishing between Wasm deploys and transfers.
#[derive(
    Copy,
    Clone,
    DataSize,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Debug,
    Display,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub enum DeployOrTransferHash {
    /// Hash of a deploy.
    #[display(fmt = "deploy {}", _0)]
    Deploy(DeployHash),
    /// Hash of a transfer.
    #[display(fmt = "transfer {}", _0)]
    Transfer(DeployHash),
}

impl DeployOrTransferHash {
    /// Gets the inner `DeployHash`.
    pub fn deploy_hash(&self) -> &DeployHash {
        match self {
            DeployOrTransferHash::Deploy(hash) | DeployOrTransferHash::Transfer(hash) => hash,
        }
    }

    /// Returns `true` if this is a transfer hash.
    pub fn is_transfer(&self) -> bool {
        matches!(self, DeployOrTransferHash::Transfer(_))
    }
}

impl From<DeployOrTransferHash> for DeployHash {
    fn from(dt_hash: DeployOrTransferHash) -> DeployHash {
        match dt_hash {
            DeployOrTransferHash::Deploy(hash) => hash,
            DeployOrTransferHash::Transfer(hash) => hash,
        }
    }
}
