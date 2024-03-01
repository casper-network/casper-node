use datasize::DataSize;
use derive_more::Display;
use serde::{Deserialize, Serialize};

use casper_types::{Deploy, DeployHash, TransactionHash};

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
)]
#[serde(deny_unknown_fields)]
pub(crate) enum DeployOrTransferHash {
    /// Hash of a deploy.
    #[display(fmt = "deploy {}", _0)]
    Deploy(DeployHash),
    /// Hash of a transfer.
    #[display(fmt = "transfer {}", _0)]
    Transfer(DeployHash),
}

impl DeployOrTransferHash {
    /// Returns the hash of `deploy` wrapped in `DeployOrTransferHash`.
    pub(crate) fn new(deploy: &Deploy) -> DeployOrTransferHash {
        if deploy.session().is_transfer() {
            DeployOrTransferHash::Transfer(*deploy.hash())
        } else {
            DeployOrTransferHash::Deploy(*deploy.hash())
        }
    }

    #[cfg(test)]
    pub(crate) fn deploy_hash(&self) -> &DeployHash {
        match self {
            DeployOrTransferHash::Deploy(hash) | DeployOrTransferHash::Transfer(hash) => hash,
        }
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

impl From<DeployOrTransferHash> for TransactionHash {
    fn from(dt_hash: DeployOrTransferHash) -> TransactionHash {
        match dt_hash {
            DeployOrTransferHash::Transfer(hash) | DeployOrTransferHash::Deploy(hash) => {
                TransactionHash::Deploy(hash)
            }
        }
    }
}
