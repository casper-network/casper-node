#[cfg(any(feature = "std", test))]
mod account_and_secret_key;
mod deploy;
mod transaction_approvals_hash;
mod transaction_hash;
mod transaction_id;
mod transaction_v1;

use alloc::{collections::BTreeSet, vec::Vec};
use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, PublicKey, Timestamp,
};
#[cfg(any(feature = "std", test))]
use account_and_secret_key::AccountAndSecretKey;
pub use deploy::{
    runtime_args, Deploy, DeployApproval, DeployApprovalsHash, DeployConfigurationFailure,
    DeployDecodeFromJsonError, DeployError, DeployExcessiveSizeError, DeployFootprint, DeployHash,
    DeployHeader, DeployId, EntityIdentifier, ExecutableDeployItem, ExecutableDeployItemIdentifier,
    PackageIdentifier, TransferTarget,
};
#[cfg(any(feature = "std", test))]
pub use deploy::{DeployBuilder, DeployBuilderError};
pub use transaction_approvals_hash::TransactionApprovalsHash;
pub use transaction_hash::TransactionHash;
pub use transaction_id::TransactionId;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
pub use transaction_v1::TestTransactionV1Builder;
pub use transaction_v1::{
    AuctionTransactionV1, DirectCallV1, NativeTransactionV1, PricingModeV1, TransactionV1,
    TransactionV1Approval, TransactionV1ApprovalsHash, TransactionV1ConfigFailure,
    TransactionV1DecodeFromJsonError, TransactionV1Error, TransactionV1ExcessiveSizeError,
    TransactionV1Hash, TransactionV1Header, TransactionV1Kind, UserlandTransactionV1,
};
#[cfg(any(feature = "std", test))]
pub use transaction_v1::{TransactionV1Builder, TransactionV1BuilderError};

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;

/// A versioned wrapper for a transaction or deploy.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum Transaction {
    /// A deploy.
    Deploy(Deploy),
    /// A version 1 transaction.
    V1(TransactionV1),
}

impl Transaction {
    /// Returns the `TransactionHash` identifying this transaction.
    pub fn hash(&self) -> TransactionHash {
        match self {
            Transaction::Deploy(deploy) => TransactionHash::from(*deploy.hash()),
            Transaction::V1(txn) => TransactionHash::from(*txn.hash()),
        }
    }

    /// Returns the computed `TransactionId` uniquely identifying this transaction and its
    /// approvals.
    pub fn compute_id(&self) -> TransactionId {
        match self {
            Transaction::Deploy(deploy) => {
                let deploy_hash = *deploy.hash();
                let approvals_hash = deploy.compute_approvals_hash().unwrap_or_else(|error| {
                    error!(%error, "failed to serialize deploy approvals");
                    DeployApprovalsHash::from(Digest::default())
                });
                TransactionId::new_deploy(deploy_hash, approvals_hash)
            }
            Transaction::V1(txn) => {
                let txn_hash = *txn.hash();
                let approvals_hash = txn.compute_approvals_hash().unwrap_or_else(|error| {
                    error!(%error, "failed to serialize transaction approvals");
                    TransactionV1ApprovalsHash::from(Digest::default())
                });
                TransactionId::new_v1(txn_hash, approvals_hash)
            }
        }
    }

    /// Returns the public key of the account providing the context in which to run the transaction.
    pub fn account(&self) -> &PublicKey {
        match self {
            Transaction::Deploy(deploy) => deploy.account(),
            Transaction::V1(txn) => txn.account(),
        }
    }

    /// Returns `true` if the transaction has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        match self {
            Transaction::Deploy(deploy) => deploy.expired(current_instant),
            Transaction::V1(txn) => txn.expired(current_instant),
        }
    }

    /// Returns the timestamp of when the transaction expires, i.e. `self.timestamp + self.ttl`.
    pub fn expires(&self) -> Timestamp {
        match self {
            Transaction::Deploy(deploy) => deploy.header().expires(),
            Transaction::V1(txn) => txn.header().expires(),
        }
    }

    /// Returns the set of account hashes corresponding to the public keys of the approvals.
    pub fn signers(&self) -> BTreeSet<AccountHash> {
        match self {
            Transaction::Deploy(deploy) => deploy
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
            Transaction::V1(txn) => txn
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
        }
    }
}

impl From<Deploy> for Transaction {
    fn from(deploy: Deploy) -> Self {
        Self::Deploy(deploy)
    }
}

impl From<TransactionV1> for Transaction {
    fn from(txn: TransactionV1) -> Self {
        Self::V1(txn)
    }
}

impl ToBytes for Transaction {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            Transaction::Deploy(deploy) => {
                DEPLOY_TAG.write_bytes(writer)?;
                deploy.write_bytes(writer)
            }
            Transaction::V1(txn) => {
                V1_TAG.write_bytes(writer)?;
                txn.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Transaction::Deploy(deploy) => deploy.serialized_length(),
                Transaction::V1(txn) => txn.serialized_length(),
            }
    }
}

impl FromBytes for Transaction {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            DEPLOY_TAG => {
                let (deploy, remainder) = Deploy::from_bytes(remainder)?;
                Ok((Transaction::Deploy(deploy), remainder))
            }
            V1_TAG => {
                let (txn, remainder) = TransactionV1::from_bytes(remainder)?;
                Ok((Transaction::V1(txn), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Display for Transaction {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Transaction::Deploy(deploy) => Display::fmt(deploy, formatter),
            Transaction::V1(txn) => Display::fmt(txn, formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn json_roundtrip() {
        let rng = &mut TestRng::new();

        let transaction = Transaction::from(Deploy::random(rng));
        let json_string = serde_json::to_string_pretty(&transaction).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(transaction, decoded);

        let transaction = Transaction::from(TransactionV1::random(rng));
        let json_string = serde_json::to_string_pretty(&transaction).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(transaction, decoded);
    }

    #[test]
    fn bincode_roundtrip() {
        let rng = &mut TestRng::new();

        let transaction = Transaction::from(Deploy::random(rng));
        let serialized = bincode::serialize(&transaction).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(transaction, deserialized);

        let transaction = Transaction::from(TransactionV1::random(rng));
        let serialized = bincode::serialize(&transaction).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(transaction, deserialized);
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let transaction = Transaction::from(Deploy::random(rng));
        bytesrepr::test_serialization_roundtrip(&transaction);

        let transaction = Transaction::from(TransactionV1::random(rng));
        bytesrepr::test_serialization_roundtrip(&transaction);
    }
}
