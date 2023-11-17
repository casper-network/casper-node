use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TransactionHash;

#[derive(
    Copy, Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug,
)]
pub(crate) enum TypedTransactionHash {
    Transfer(TransactionHash),
    Staking(TransactionHash),
    InstallUpgrade(TransactionHash),
    Standard(TransactionHash),
}

impl From<TypedTransactionHash> for TransactionHash {
    fn from(typed_txn_hash: TypedTransactionHash) -> TransactionHash {
        match typed_txn_hash {
            TypedTransactionHash::Transfer(txn_hash)
            | TypedTransactionHash::Staking(txn_hash)
            | TypedTransactionHash::InstallUpgrade(txn_hash)
            | TypedTransactionHash::Standard(txn_hash) => txn_hash,
        }
    }
}
