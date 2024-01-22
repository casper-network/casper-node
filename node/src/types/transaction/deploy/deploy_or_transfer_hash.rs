use datasize::DataSize;
use derive_more::Display;
use serde::{Deserialize, Serialize};

use casper_types::{Deploy, DeployHash, Transaction, TransactionHash};

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
    Deploy(TransactionHash),
    /// Hash of a transfer.
    #[display(fmt = "transfer {}", _0)]
    Transfer(TransactionHash), // TODO[RC]: Should this remain as DeployHash?
}

impl DeployOrTransferHash {
    /// Returns the hash of `deploy` wrapped in `DeployOrTransferHash`.
    pub(crate) fn new(transaction: &Transaction) -> DeployOrTransferHash {
        match transaction {
            Transaction::Deploy(deploy) if deploy.session().is_transfer() => {
                DeployOrTransferHash::Transfer(transaction.hash())
            }
            Transaction::V1(v1) if v1.is_transfer() => {
                DeployOrTransferHash::Transfer(transaction.hash())
            }
            Transaction::V1(_) | Transaction::Deploy(_) => {
                DeployOrTransferHash::Deploy(transaction.hash())
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn deploy_hash(&self) -> &TransactionHash {
        match self {
            DeployOrTransferHash::Deploy(hash) | DeployOrTransferHash::Transfer(hash) => hash,
        }
    }
}

impl From<DeployOrTransferHash> for TransactionHash {
    fn from(dt_hash: DeployOrTransferHash) -> TransactionHash {
        match dt_hash {
            DeployOrTransferHash::Deploy(hash) => hash,
            DeployOrTransferHash::Transfer(hash) => hash,
        }
    }
}
