use datasize::DataSize;
use derive_more::Display;
use serde::{Deserialize, Serialize};

use casper_types::{Deploy, DeployHash, TransactionHash, TransactionV1, TransactionV1Hash};

// TODO[RC]: To separate file
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
pub(crate) enum TransactionV1OrTransferV1Hash {
    /// Hash of a deploy.
    #[display(fmt = "transaction {}", _0)]
    Transaction(TransactionV1Hash),
    /// Hash of a transfer.
    #[display(fmt = "transfer V1 {}", _0)]
    Transfer(TransactionV1Hash),
}

impl TransactionV1OrTransferV1Hash {
    /// Returns the hash of `transaction` wrapped in `TransactionV1OrTransferV1Hash`.
    pub(crate) fn new(transaction: &TransactionV1) -> TransactionV1OrTransferV1Hash {
        if transaction.is_transfer() {
            TransactionV1OrTransferV1Hash::Transfer(*transaction.hash())
        } else {
            TransactionV1OrTransferV1Hash::Transaction(*transaction.hash())
        }
    }
}

impl From<TransactionV1OrTransferV1Hash> for TransactionV1Hash {
    fn from(dt_hash: TransactionV1OrTransferV1Hash) -> TransactionV1Hash {
        match dt_hash {
            TransactionV1OrTransferV1Hash::Transaction(hash) => hash,
            TransactionV1OrTransferV1Hash::Transfer(hash) => hash,
        }
    }
}

impl From<TransactionV1OrTransferV1Hash> for TransactionHash {
    fn from(dt_hash: TransactionV1OrTransferV1Hash) -> TransactionHash {
        match dt_hash {
            TransactionV1OrTransferV1Hash::Transaction(hash) => TransactionHash::V1(hash),
            TransactionV1OrTransferV1Hash::Transfer(hash) => TransactionHash::V1(hash),
        }
    }
}

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
