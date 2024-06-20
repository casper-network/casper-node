mod addressable_entity_identifier;
mod approval;
mod approvals_hash;
mod deploy;
mod error;
mod execution_info;
mod initiator_addr;
#[cfg(any(feature = "std", test))]
mod initiator_addr_and_secret_key;
mod package_identifier;
mod pricing_mode;
mod runtime_args;
mod transaction_category;
mod transaction_entry_point;
mod transaction_hash;
mod transaction_header;
mod transaction_id;
mod transaction_invocation_target;
mod transaction_runtime;
mod transaction_scheduling;
mod transaction_target;
mod transaction_v1;
mod transfer_target;

use alloc::{collections::BTreeSet, vec::Vec};
use core::fmt::{self, Debug, Display, Formatter};
#[cfg(any(feature = "std", test))]
use std::hash::Hash;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
use tracing::error;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
pub use crate::transaction::transaction_v1::TransactionArgs;
#[cfg(feature = "json-schema")]
use crate::URef;
use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, Phase, SecretKey, TimeDiff, Timestamp,
};
#[cfg(any(feature = "std", test))]
use crate::{Chainspec, Gas, Motes};
pub use addressable_entity_identifier::AddressableEntityIdentifier;
pub use approval::Approval;
pub use approvals_hash::ApprovalsHash;
pub use deploy::{
    Deploy, DeployDecodeFromJsonError, DeployError, DeployExcessiveSizeError, DeployHash,
    DeployHeader, DeployId, ExecutableDeployItem, ExecutableDeployItemIdentifier, InvalidDeploy,
};
#[cfg(any(feature = "std", test))]
pub use deploy::{DeployBuilder, DeployBuilderError};
pub use error::InvalidTransaction;
pub use execution_info::ExecutionInfo;
pub use initiator_addr::InitiatorAddr;
#[cfg(any(feature = "std", test))]
use initiator_addr_and_secret_key::InitiatorAddrAndSecretKey;
pub use package_identifier::PackageIdentifier;
pub use pricing_mode::PricingMode;
pub use runtime_args::{NamedArg, RuntimeArgs};
pub use transaction_entry_point::TransactionEntryPoint;
pub use transaction_hash::TransactionHash;
pub use transaction_header::TransactionHeader;
pub use transaction_id::TransactionId;
pub use transaction_invocation_target::TransactionInvocationTarget;
pub use transaction_runtime::TransactionRuntime;
pub use transaction_scheduling::TransactionScheduling;
pub use transaction_target::TransactionTarget;
pub use transaction_v1::{
    InvalidTransactionV1, TransactionCategory, TransactionV1, TransactionV1Body,
    TransactionV1DecodeFromJsonError, TransactionV1Error, TransactionV1ExcessiveSizeError,
    TransactionV1Hash, TransactionV1Header,
};
#[cfg(any(feature = "std", test))]
pub use transaction_v1::{TransactionV1Builder, TransactionV1BuilderError};
pub use transfer_target::TransferTarget;

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;

#[cfg(feature = "json-schema")]
pub(super) static TRANSACTION: Lazy<Transaction> = Lazy::new(|| {
    let secret_key = SecretKey::example();
    let source = URef::from_formatted_str(
        "uref-0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a-007",
    )
    .unwrap();
    let target = URef::from_formatted_str(
        "uref-1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b-000",
    )
    .unwrap();
    let id = Some(999);

    let v1_txn = TransactionV1Builder::new_transfer(30_000_000_000_u64, Some(source), target, id)
        .unwrap()
        .with_chain_name("casper-example")
        .with_timestamp(*Timestamp::example())
        .with_ttl(TimeDiff::from_seconds(3_600))
        .with_secret_key(secret_key)
        .build()
        .unwrap();
    Transaction::V1(v1_txn)
});

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
    #[cfg_attr(any(feature = "std", test), serde(rename = "Version1"))]
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

    /// Body hash.
    pub fn body_hash(&self) -> Digest {
        match self {
            Transaction::Deploy(deploy) => *deploy.header().body_hash(),
            Transaction::V1(v1) => *v1.header().body_hash(),
        }
    }

    /// Size estimate.
    pub fn size_estimate(&self) -> usize {
        match self {
            Transaction::Deploy(deploy) => deploy.serialized_length(),
            Transaction::V1(v1) => v1.serialized_length(),
        }
    }

    /// Timestamp.
    pub fn timestamp(&self) -> Timestamp {
        match self {
            Transaction::Deploy(deploy) => deploy.header().timestamp(),
            Transaction::V1(v1) => v1.header().timestamp(),
        }
    }

    /// Time to live.
    pub fn ttl(&self) -> TimeDiff {
        match self {
            Transaction::Deploy(deploy) => deploy.header().ttl(),
            Transaction::V1(v1) => v1.header().ttl(),
        }
    }

    /// Returns `Ok` if the given transaction is valid. Verification procedure is delegated to the
    /// implementation of the particular variant of the transaction.
    pub fn verify(&self) -> Result<(), InvalidTransaction> {
        match self {
            Transaction::Deploy(deploy) => deploy.is_valid().map_err(Into::into),
            Transaction::V1(v1) => v1.verify().map_err(Into::into),
        }
    }

    /// Adds a signature of this transaction's hash to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey) {
        match self {
            Transaction::Deploy(deploy) => deploy.sign(secret_key),
            Transaction::V1(v1) => v1.sign(secret_key),
        }
    }

    /// Returns the `Approval`s for this transaction.
    pub fn approvals(&self) -> BTreeSet<Approval> {
        match self {
            Transaction::Deploy(deploy) => deploy.approvals().clone(),
            Transaction::V1(v1) => v1.approvals().clone(),
        }
    }

    /// Returns the header.
    pub fn header(&self) -> TransactionHeader {
        match self {
            Transaction::Deploy(deploy) => TransactionHeader::Deploy(deploy.header().clone()),
            Transaction::V1(transaction) => TransactionHeader::V1(transaction.header().clone()),
        }
    }

    /// Returns the computed approvals hash identifying this transaction's approvals.
    pub fn compute_approvals_hash(&self) -> Result<ApprovalsHash, bytesrepr::Error> {
        let approvals_hash = match self {
            Transaction::Deploy(deploy) => deploy.compute_approvals_hash()?,
            Transaction::V1(txn) => txn.compute_approvals_hash()?,
        };
        Ok(approvals_hash)
    }

    /// Turns `self` into an invalid `Transaction` by clearing the `chain_name`, invalidating the
    /// transaction hash.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn invalidate(&mut self) {
        match self {
            Transaction::Deploy(deploy) => deploy.invalidate(),
            Transaction::V1(v1) => v1.invalidate(),
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
                    ApprovalsHash::from(Digest::default())
                });
                TransactionId::new(TransactionHash::Deploy(deploy_hash), approvals_hash)
            }
            Transaction::V1(txn) => {
                let txn_hash = *txn.hash();
                let approvals_hash = txn.compute_approvals_hash().unwrap_or_else(|error| {
                    error!(%error, "failed to serialize transaction approvals");
                    ApprovalsHash::from(Digest::default())
                });
                TransactionId::new(TransactionHash::V1(txn_hash), approvals_hash)
            }
        }
    }

    /// Returns the address of the initiator of the transaction.
    pub fn initiator_addr(&self) -> InitiatorAddr {
        match self {
            Transaction::Deploy(deploy) => InitiatorAddr::PublicKey(deploy.account().clone()),
            Transaction::V1(txn) => txn.initiator_addr().clone(),
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

    // This method is not intended to be used by third party crates.
    //
    // It is required to allow finalized approvals to be injected after reading a `Deploy` from
    // storage.
    #[doc(hidden)]
    pub fn with_approvals(self, approvals: BTreeSet<Approval>) -> Self {
        match self {
            Transaction::Deploy(deploy) => Transaction::Deploy(deploy.with_approvals(approvals)),
            Transaction::V1(transaction_v1) => {
                Transaction::V1(transaction_v1.with_approvals(approvals))
            }
        }
    }

    /// Returns `true` if `self` represents a native transfer deploy or a native V1 transaction.
    pub fn is_native(&self) -> bool {
        match self {
            Transaction::Deploy(deploy) => deploy.is_transfer(),
            Transaction::V1(v1_txn) => *v1_txn.target() == TransactionTarget::Native,
        }
    }

    /// Is this a transaction that should be sent to the v1 execution engine?
    pub fn is_v1_wasm(&self) -> bool {
        match self {
            Transaction::Deploy(deploy) => !deploy.is_transfer(),
            Transaction::V1(v1) => v1.is_v1_wasm(),
        }
    }

    /// Is this a transaction that should be sent to the v2 execution engine?
    pub fn is_v2_wasm(&self) -> bool {
        match self {
            Transaction::Deploy(deploy) => !deploy.is_transfer(),
            Transaction::V1(v1) => v1.is_v2_wasm(),
        }
    }

    pub fn as_transaction_v1(&self) -> Option<&TransactionV1> {
        match self {
            Transaction::Deploy(_) => None,
            Transaction::V1(v1) => Some(v1),
        }
    }

    /// Should this transaction use standard payment processing?
    pub fn is_standard_payment(&self) -> bool {
        match self {
            Transaction::Deploy(deploy) => deploy.payment().is_standard_payment(Phase::Payment),
            Transaction::V1(v1) => {
                if let PricingMode::Classic {
                    standard_payment, ..
                } = v1.pricing_mode()
                {
                    *standard_payment
                } else {
                    true
                }
            }
        }
    }

    pub fn is_gas_limited(&self) -> Option<bool> {
        match self {
            Transaction::Deploy(_) => None,
            Transaction::V1(v1) => {
                if let PricingMode::GasLimited { .. } = v1.pricing_mode() {
                    Some(true)
                } else {
                    Some(false)
                }
            }
        }
    }

    /// Should this transaction start in the initiating accounts context?
    pub fn is_account_session(&self) -> bool {
        match self {
            Transaction::Deploy(deploy) => deploy.is_account_session(),
            Transaction::V1(v1) => v1.is_account_session(),
        }
    }

    /// Authorization keys.
    pub fn authorization_keys(&self) -> BTreeSet<AccountHash> {
        match self {
            Transaction::Deploy(deploy) => deploy
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
            Transaction::V1(transaction_v1) => transaction_v1
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
        }
    }

    /// The session args.
    pub fn session_args(&self) -> Option<&RuntimeArgs> {
        match self {
            Transaction::Deploy(deploy) => Some(deploy.session().args()),
            Transaction::V1(transaction_v1) => transaction_v1.body().args().as_named(),
        }
    }

    /// The entry point.
    pub fn entry_point(&self) -> TransactionEntryPoint {
        match self {
            Transaction::Deploy(deploy) => deploy.session().entry_point_name().into(),
            Transaction::V1(transaction_v1) => transaction_v1.entry_point().clone(),
        }
    }

    /// The transaction category.
    pub fn transaction_category(&self) -> u8 {
        match self {
            Transaction::Deploy(deploy) => {
                if deploy.is_transfer() {
                    TransactionCategory::Mint as u8
                } else {
                    TransactionCategory::Large as u8
                }
            }
            Transaction::V1(v1) => v1.transaction_category(),
        }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &TRANSACTION
    }

    /// Returns a random, valid but possibly expired transaction.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            Transaction::Deploy(Deploy::random_valid_native_transfer(rng))
        } else {
            Transaction::V1(TransactionV1::random(rng))
        }
    }
}

/// Calculates gas limit.
#[cfg(any(feature = "std", test))]
pub trait GasLimited {
    /// The error type.
    type Error;

    /// The minimum allowed gas price (aka the floor).
    const GAS_PRICE_FLOOR: u8 = 1;

    /// Returns a gas cost based upon the gas_limit, the gas price,
    /// and the chainspec settings.
    fn gas_cost(&self, chainspec: &Chainspec, gas_price: u8) -> Result<Motes, Self::Error>;

    /// Returns the gas / computation limit prior to execution.
    fn gas_limit(&self, chainspec: &Chainspec) -> Result<Gas, Self::Error>;

    /// Returns the gas price tolerance.
    fn gas_price_tolerance(&self) -> Result<u8, Self::Error>;
}

#[cfg(any(feature = "std", test))]
impl GasLimited for Transaction {
    type Error = InvalidTransaction;

    fn gas_cost(&self, chainspec: &Chainspec, gas_price: u8) -> Result<Motes, Self::Error> {
        match self {
            Transaction::Deploy(deploy) => deploy
                .gas_cost(chainspec, gas_price)
                .map_err(InvalidTransaction::from),
            Transaction::V1(v1) => v1
                .gas_cost(chainspec, gas_price)
                .map_err(InvalidTransaction::from),
        }
    }

    fn gas_limit(&self, chainspec: &Chainspec) -> Result<Gas, Self::Error> {
        match self {
            Transaction::Deploy(deploy) => deploy
                .gas_limit(chainspec)
                .map_err(InvalidTransaction::from),
            Transaction::V1(v1) => v1.gas_limit(chainspec).map_err(InvalidTransaction::from),
        }
    }

    fn gas_price_tolerance(&self) -> Result<u8, Self::Error> {
        match self {
            Transaction::Deploy(deploy) => deploy
                .gas_price_tolerance()
                .map_err(InvalidTransaction::from),
            Transaction::V1(v1) => v1.gas_price_tolerance().map_err(InvalidTransaction::from),
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

/// Proptest generators for [`Transaction`].
#[cfg(any(feature = "testing", feature = "gens", test))]
pub mod gens {
    use proptest::{
        array,
        prelude::{Arbitrary, Strategy},
    };

    use super::*;

    pub fn deploy_hash_arb() -> impl Strategy<Value = DeployHash> {
        array::uniform32(<u8>::arbitrary()).prop_map(DeployHash::from_raw)
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
