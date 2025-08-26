mod addressable_entity_identifier;
mod approval;
mod approvals_hash;
mod deploy;
mod error;
mod execution_info;
mod initiator_addr;
#[cfg(any(feature = "std", test, feature = "testing"))]
mod initiator_addr_and_secret_key;
mod package_identifier;
mod pricing_mode;
mod runtime_args;
mod serialization;
mod transaction_entry_point;
mod transaction_hash;
mod transaction_id;
mod transaction_invocation_target;
mod transaction_scheduling;
mod transaction_target;
mod transaction_v1;
mod transfer_target;

#[cfg(feature = "json-schema")]
use crate::URef;
use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt::{self, Debug, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{de, ser, Deserializer, Serializer};
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "std", test))]
use serde_bytes::ByteBuf;
#[cfg(any(feature = "std", test))]
use std::hash::Hash;
#[cfg(any(feature = "std", test))]
use thiserror::Error;
use tracing::error;
#[cfg(any(feature = "std", test))]
pub use transaction_v1::calculate_transaction_lane;
#[cfg(any(feature = "std", test))]
use transaction_v1::TransactionV1Json;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, Phase, SecretKey, TimeDiff, Timestamp,
};
#[cfg(any(feature = "std", test))]
use crate::{Chainspec, Gas, Motes, TransactionV1Config};
pub use addressable_entity_identifier::AddressableEntityIdentifier;
pub use approval::Approval;
pub use approvals_hash::ApprovalsHash;
#[cfg(any(feature = "std", test))]
pub use deploy::calculate_lane_id_for_deploy;
pub use deploy::{
    Deploy, DeployDecodeFromJsonError, DeployError, DeployExcessiveSizeError, DeployHash,
    DeployHeader, DeployId, ExecutableDeployItem, ExecutableDeployItemIdentifier, InvalidDeploy,
};
pub use error::InvalidTransaction;
pub use execution_info::ExecutionInfo;
pub use initiator_addr::InitiatorAddr;
#[cfg(any(feature = "std", feature = "testing", test))]
pub(crate) use initiator_addr_and_secret_key::InitiatorAddrAndSecretKey;
pub use package_identifier::PackageIdentifier;
pub use pricing_mode::{PricingMode, PricingModeError};
pub use runtime_args::{NamedArg, RuntimeArgs};
pub use transaction_entry_point::TransactionEntryPoint;
pub use transaction_hash::TransactionHash;
pub use transaction_id::TransactionId;
pub use transaction_invocation_target::TransactionInvocationTarget;
pub use transaction_scheduling::TransactionScheduling;
pub use transaction_target::{TransactionRuntimeParams, TransactionTarget};
#[cfg(feature = "json-schema")]
pub(crate) use transaction_v1::arg_handling;
#[cfg(any(feature = "std", feature = "testing", feature = "gens", test))]
pub(crate) use transaction_v1::fields_container::FieldsContainer;
pub use transaction_v1::{
    InvalidTransactionV1, TransactionArgs, TransactionV1, TransactionV1DecodeFromJsonError,
    TransactionV1Error, TransactionV1ExcessiveSizeError, TransactionV1Hash, TransactionV1Payload,
};
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
    let amount = 30_000_000_000_u64;
    let args = arg_handling::new_transfer_args(amount, Some(source), target, id).unwrap();
    let container = FieldsContainer::new(
        TransactionArgs::Named(args),
        TransactionTarget::Native,
        TransactionEntryPoint::Transfer,
        TransactionScheduling::Standard,
    );
    let pricing_mode = PricingMode::Fixed {
        gas_price_tolerance: 5,
        additional_computation_factor: 0,
    };
    let initiator_addr_and_secret_key = InitiatorAddrAndSecretKey::SecretKey(secret_key);
    let v1_txn = TransactionV1::build(
        "casper-example".to_string(),
        *Timestamp::example(),
        TimeDiff::from_seconds(3_600),
        pricing_mode,
        container.to_map().unwrap(),
        initiator_addr_and_secret_key,
    );
    Transaction::V1(v1_txn)
});

/// A versioned wrapper for a transaction or deploy.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum Transaction {
    /// A deploy.
    Deploy(Deploy),
    /// A version 1 transaction.
    #[cfg_attr(
        feature = "json-schema",
        serde(rename = "Version1"),
        schemars(with = "TransactionV1Json")
    )]
    V1(TransactionV1),
}

impl Transaction {
    // Deploy variant ctor
    pub fn from_deploy(deploy: Deploy) -> Self {
        Transaction::Deploy(deploy)
    }

    // V1 variant ctor
    pub fn from_v1(v1: TransactionV1) -> Self {
        Transaction::V1(v1)
    }

    /// Returns the `TransactionHash` identifying this transaction.
    pub fn hash(&self) -> TransactionHash {
        match self {
            Transaction::Deploy(deploy) => TransactionHash::from(*deploy.hash()),
            Transaction::V1(txn) => TransactionHash::from(*txn.hash()),
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
            Transaction::V1(v1) => v1.payload().timestamp(),
        }
    }

    /// Time to live.
    pub fn ttl(&self) -> TimeDiff {
        match self {
            Transaction::Deploy(deploy) => deploy.header().ttl(),
            Transaction::V1(v1) => v1.payload().ttl(),
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

    /// Returns the computed approvals hash identifying this transaction's approvals.
    pub fn compute_approvals_hash(&self) -> Result<ApprovalsHash, bytesrepr::Error> {
        let approvals_hash = match self {
            Transaction::Deploy(deploy) => deploy.compute_approvals_hash()?,
            Transaction::V1(txn) => txn.compute_approvals_hash()?,
        };
        Ok(approvals_hash)
    }

    /// Returns the chain name for the transaction, whether it's a `Deploy` or `V1` transaction.
    pub fn chain_name(&self) -> String {
        match self {
            Transaction::Deploy(txn) => txn.chain_name().to_string(),
            Transaction::V1(txn) => txn.chain_name().to_string(),
        }
    }

    /// Checks if the transaction is a standard payment.
    ///
    /// For `Deploy` transactions, it checks if the session is a standard payment
    /// in the payment phase. For `V1` transactions, it returns the value of
    /// `standard_payment` if the pricing mode is `PaymentLimited`, otherwise it returns `true`.
    pub fn is_standard_payment(&self) -> bool {
        match self {
            Transaction::Deploy(txn) => txn.session().is_standard_payment(Phase::Payment),
            Transaction::V1(txn) => match txn.pricing_mode() {
                PricingMode::PaymentLimited {
                    standard_payment, ..
                } => *standard_payment,
                _ => true,
            },
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
            Transaction::V1(txn) => txn.payload().expires(),
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

    /// Get [`TransactionV1`]
    pub fn as_transaction_v1(&self) -> Option<&TransactionV1> {
        match self {
            Transaction::Deploy(_) => None,
            Transaction::V1(v1) => Some(v1),
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

    /// Is the transaction the legacy deploy variant.
    pub fn is_legacy_transaction(&self) -> bool {
        match self {
            Transaction::Deploy(_) => true,
            Transaction::V1(_) => false,
        }
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    /// Calcualates the gas limit for the transaction.
    pub fn gas_limit(&self, chainspec: &Chainspec, lane_id: u8) -> Result<Gas, InvalidTransaction> {
        match self {
            Transaction::Deploy(deploy) => {
                match deploy
                    .gas_limit(chainspec)
                    .map_err(InvalidTransaction::from)
                {
                    Ok(gas) => {
                        if gas.value() == crate::U512::zero() {
                            Err(InvalidTransaction::Deploy(
                                InvalidDeploy::InvalidPaymentAmount,
                            ))
                        } else {
                            Ok(gas)
                        }
                    }
                    Err(err) => Err(err),
                }
            }
            Transaction::V1(v1) => {
                if let Ok(TransactionTarget::Native) = v1.get_transaction_target() {
                    // retro-compatibility for incentivized native transfer cost
                    if let Ok(TransactionEntryPoint::Transfer) = v1.get_transaction_entry_point() {
                        let gas = Gas::new(chainspec.system_costs_config.mint_costs().transfer);
                        return Ok(gas);
                    };
                }

                let pricing_mode = v1.pricing_mode();
                match pricing_mode
                    .gas_limit(chainspec, lane_id)
                    .map_err(InvalidTransaction::from)
                {
                    Ok(gas) => {
                        // the transaction acceptor enforces this on an actual network,
                        // rejecting 0 payment txn's right away.
                        // however, direct tests don't engage the acceptor.
                        // so, also checking here so those tests are consistent
                        // and also defense in depth
                        if gas.value() == crate::U512::zero() {
                            Err(InvalidTransaction::V1(
                                InvalidTransactionV1::InvalidPaymentAmount,
                            ))
                        } else {
                            Ok(gas)
                        }
                    }
                    Err(err) => Err(err),
                }
            }
        }
    }

    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    /// Returns a gas cost based upon the gas_limit, the gas price,
    /// and the chainspec settings.
    pub fn gas_cost(
        &self,
        chainspec: &Chainspec,
        lane_id: u8,
        gas_price: u8,
    ) -> Result<Motes, InvalidTransaction> {
        match self {
            Transaction::Deploy(deploy) => deploy
                .gas_cost(chainspec, gas_price)
                .map_err(InvalidTransaction::from),
            Transaction::V1(v1) => {
                if let Ok(TransactionTarget::Native) = v1.get_transaction_target() {
                    // retro-compatibility for incentivized native transfer cost
                    if let Ok(TransactionEntryPoint::Transfer) = v1.get_transaction_entry_point() {
                        return Ok(Motes::new(
                            chainspec.system_costs_config.mint_costs().transfer,
                        ));
                    };
                }
                let pricing_mode = v1.pricing_mode();
                pricing_mode
                    .gas_cost(chainspec, lane_id, gas_price)
                    .map_err(InvalidTransaction::from)
            }
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

#[cfg(any(feature = "std", test))]
impl Serialize for Transaction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            TransactionJson::try_from(self.clone())
                .map_err(|error| ser::Error::custom(format!("{:?}", error)))?
                .serialize(serializer)
        } else {
            let bytes = self
                .to_bytes()
                .map_err(|error| ser::Error::custom(format!("{:?}", error)))?;
            ByteBuf::from(bytes).serialize(serializer)
        }
    }
}

#[cfg(any(feature = "std", test))]
impl<'de> Deserialize<'de> for Transaction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let json_helper = TransactionJson::deserialize(deserializer)?;
            Transaction::try_from(json_helper)
                .map_err(|error| de::Error::custom(format!("{:?}", error)))
        } else {
            let bytes = ByteBuf::deserialize(deserializer)?.into_vec();
            bytesrepr::deserialize::<Transaction>(bytes)
                .map_err(|error| de::Error::custom(format!("{:?}", error)))
        }
    }
}

/// A util structure to json-serialize a transaction.
#[cfg(any(feature = "std", test))]
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
enum TransactionJson {
    /// A deploy.
    Deploy(Deploy),
    /// A version 1 transaction.
    #[serde(rename = "Version1")]
    V1(TransactionV1Json),
}

#[cfg(any(feature = "std", test))]
#[derive(Error, Debug)]
enum TransactionJsonError {
    #[error("{0}")]
    FailedToMap(String),
}

#[cfg(any(feature = "std", test))]
impl TryFrom<TransactionJson> for Transaction {
    type Error = TransactionJsonError;
    fn try_from(transaction: TransactionJson) -> Result<Self, Self::Error> {
        match transaction {
            TransactionJson::Deploy(deploy) => Ok(Transaction::Deploy(deploy)),
            TransactionJson::V1(v1) => {
                TransactionV1::try_from(v1)
                    .map(Transaction::V1)
                    .map_err(|error| {
                        TransactionJsonError::FailedToMap(format!(
                            "Failed to map TransactionJson::V1 to Transaction::V1, err: {}",
                            error
                        ))
                    })
            }
        }
    }
}

#[cfg(any(feature = "std", test))]
impl TryFrom<Transaction> for TransactionJson {
    type Error = TransactionJsonError;
    fn try_from(transaction: Transaction) -> Result<Self, Self::Error> {
        match transaction {
            Transaction::Deploy(deploy) => Ok(TransactionJson::Deploy(deploy)),
            Transaction::V1(v1) => TransactionV1Json::try_from(v1)
                .map(TransactionJson::V1)
                .map_err(|error| {
                    TransactionJsonError::FailedToMap(format!(
                        "Failed to map Transaction::V1 to TransactionJson::V1, err: {}",
                        error
                    ))
                }),
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

#[cfg(any(feature = "std", test))]
pub(crate) enum GetLaneError {
    NoLaneMatch,
    PricingModeNotSupported,
}

#[cfg(any(feature = "std", test))]
impl From<GetLaneError> for InvalidTransactionV1 {
    fn from(value: GetLaneError) -> Self {
        match value {
            GetLaneError::NoLaneMatch => InvalidTransactionV1::NoLaneMatch,
            GetLaneError::PricingModeNotSupported => InvalidTransactionV1::PricingModeNotSupported,
        }
    }
}

#[cfg(any(feature = "std", test))]
impl From<GetLaneError> for InvalidDeploy {
    fn from(value: GetLaneError) -> Self {
        match value {
            GetLaneError::NoLaneMatch => InvalidDeploy::NoLaneMatch,
            GetLaneError::PricingModeNotSupported => InvalidDeploy::PricingModeNotSupported,
        }
    }
}

#[cfg(any(feature = "std", test))]
pub(crate) fn get_lane_for_non_install_wasm(
    config: &TransactionV1Config,
    pricing_mode: &PricingMode,
    transaction_size: u64,
    runtime_args_size: u64,
) -> Result<u8, GetLaneError> {
    match pricing_mode {
        PricingMode::PaymentLimited { payment_amount, .. } => config
            .get_wasm_lane_id_by_payment_limited(
                *payment_amount,
                transaction_size,
                runtime_args_size,
            )
            .ok_or(GetLaneError::NoLaneMatch),
        PricingMode::Fixed {
            additional_computation_factor,
            ..
        } => config
            .get_wasm_lane_id_by_size(
                transaction_size,
                *additional_computation_factor,
                runtime_args_size,
            )
            .ok_or(GetLaneError::NoLaneMatch),
        PricingMode::Prepaid { .. } => Err(GetLaneError::PricingModeNotSupported),
    }
}

/// Proptest generators for [`Transaction`].
#[cfg(any(feature = "testing", feature = "gens", test))]
pub mod gens {
    use super::*;
    use proptest::{
        array,
        prelude::{Arbitrary, Strategy},
    };

    /// Generates a random `DeployHash` for testing purposes.
    ///
    /// This function is used to generate random `DeployHash` values for testing purposes.
    /// It produces a proptest `Strategy` that can be used to generate arbitrary `DeployHash`
    /// values.
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

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::{
        bytesrepr,
        gens::{legal_transaction_arb, transaction_arb},
    };
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn bytesrepr_roundtrip(transaction in transaction_arb()) {
            bytesrepr::test_serialization_roundtrip(&transaction);
        }

        #[test]
        fn json_roundtrip(transaction in legal_transaction_arb()) {
            let json_string = serde_json::to_string_pretty(&transaction).unwrap();
            let decoded = serde_json::from_str::<Transaction>(&json_string).unwrap();
            assert_eq!(transaction, decoded);
        }
    }
}
