mod meta_deploy;
mod meta_transaction_v1;
mod transaction_header;
use casper_execution_engine::engine_state::{SessionDataDeploy, SessionDataV1, SessionInputData};
#[cfg(test)]
use casper_types::InvalidTransactionV1;
use casper_types::{
    account::AccountHash, bytesrepr::ToBytes, Approval, Chainspec, Digest, ExecutableDeployItem,
    Gas, GasLimited, HashAddr, InitiatorAddr, InvalidTransaction, PackageAddr, Phase,
    PricingHandling, PricingMode, TimeDiff, Timestamp, Transaction, TransactionArgs,
    TransactionConfig, TransactionEntryPoint, TransactionHash, TransactionInvocationTarget,
    TransactionRuntimeParams, TransactionTarget, INSTALL_UPGRADE_LANE_ID,
};
use core::fmt::{self, Debug, Display, Formatter};
use meta_deploy::MetaDeploy;
pub(crate) use meta_transaction_v1::MetaTransactionV1;
use serde::Serialize;
use std::{borrow::Cow, collections::BTreeSet};
pub(crate) use transaction_header::*;

#[cfg(test)]
use super::fields_container::{ARGS_MAP_KEY, ENTRY_POINT_MAP_KEY, TARGET_MAP_KEY};

#[derive(Clone, Debug, Serialize)]
pub(crate) enum MetaTransaction {
    Deploy(MetaDeploy),
    V1(MetaTransactionV1),
}

impl MetaTransaction {
    /// Returns the `TransactionHash` identifying this transaction.
    pub(crate) fn hash(&self) -> TransactionHash {
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                TransactionHash::from(*meta_deploy.deploy().hash())
            }
            MetaTransaction::V1(txn) => TransactionHash::from(*txn.hash()),
        }
    }

    /// Timestamp.
    pub(crate) fn timestamp(&self) -> Timestamp {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.deploy().header().timestamp(),
            MetaTransaction::V1(v1) => v1.timestamp(),
        }
    }

    /// Time to live.
    pub(crate) fn ttl(&self) -> TimeDiff {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.deploy().header().ttl(),
            MetaTransaction::V1(v1) => v1.ttl(),
        }
    }

    /// Returns the `Approval`s for this transaction.
    pub(crate) fn approvals(&self) -> BTreeSet<Approval> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.deploy().approvals().clone(),
            MetaTransaction::V1(v1) => v1.approvals().clone(),
        }
    }

    /// Returns the address of the initiator of the transaction.
    pub(crate) fn initiator_addr(&self) -> &InitiatorAddr {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.initiator_addr(),
            MetaTransaction::V1(txn) => txn.initiator_addr(),
        }
    }

    /// Returns the set of account hashes corresponding to the public keys of the approvals.
    pub(crate) fn signers(&self) -> BTreeSet<AccountHash> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
            MetaTransaction::V1(txn) => txn
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
        }
    }

    /// Should this transaction use standard payment processing?
    pub(crate) fn is_standard_payment(&self) -> bool {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .payment()
                .is_standard_payment(Phase::Payment),
            MetaTransaction::V1(v1) => {
                if let PricingMode::PaymentLimited {
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

    /// Should this transaction use custom payment processing?
    pub(crate) fn is_custom_payment(&self) -> bool {
        match self {
            MetaTransaction::Deploy(meta_deploy) => !meta_deploy
                .deploy()
                .payment()
                .is_standard_payment(Phase::Payment),
            MetaTransaction::V1(v1) => {
                if let PricingMode::PaymentLimited {
                    standard_payment, ..
                } = v1.pricing_mode()
                {
                    !*standard_payment
                } else {
                    false
                }
            }
        }
    }

    /// Authorization keys.
    pub(crate) fn authorization_keys(&self) -> BTreeSet<AccountHash> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
            MetaTransaction::V1(transaction_v1) => transaction_v1
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
        }
    }

    /// The session args.
    pub(crate) fn session_args(&self) -> Cow<TransactionArgs> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => Cow::Owned(TransactionArgs::Named(
                meta_deploy.deploy().session().args().clone(),
            )),
            MetaTransaction::V1(transaction_v1) => Cow::Borrowed(transaction_v1.args()),
        }
    }

    /// The entry point.
    pub(crate) fn entry_point(&self) -> TransactionEntryPoint {
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                meta_deploy.deploy().session().entry_point_name().into()
            }
            MetaTransaction::V1(transaction_v1) => transaction_v1.entry_point().clone(),
        }
    }

    /// The transaction lane.
    pub(crate) fn transaction_lane(&self) -> u8 {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.lane_id(),
            MetaTransaction::V1(v1) => v1.lane_id(),
        }
    }

    /// Returns the gas price tolerance.
    pub(crate) fn gas_price_tolerance(&self) -> Result<u8, InvalidTransaction> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .gas_price_tolerance()
                .map_err(InvalidTransaction::from),
            MetaTransaction::V1(v1) => Ok(v1.gas_price_tolerance()),
        }
    }

    pub(crate) fn gas_limit(&self, chainspec: &Chainspec) -> Result<Gas, InvalidTransaction> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .gas_limit(chainspec)
                .map_err(InvalidTransaction::from),
            MetaTransaction::V1(v1) => v1.gas_limit(chainspec),
        }
    }

    /// Does this transaction provide the hash addr for a specific contract to invoke directly?
    pub(crate) fn is_contract_by_hash_invocation(&self) -> bool {
        self.contract_direct_address().is_some()
    }

    /// Returns a `hash_addr` for a targeted contract, if known.
    pub(crate) fn contract_direct_address(&self) -> Option<(HashAddr, String)> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                if let ExecutableDeployItem::StoredContractByHash {
                    hash, entry_point, ..
                } = meta_deploy.session()
                {
                    return Some((hash.value(), entry_point.clone()));
                }
            }
            MetaTransaction::V1(v1) => {
                return v1.contract_direct_address();
            }
        }
        None
    }

    /// Create a new `MetaTransaction` from a `Transaction`.
    pub(crate) fn from_transaction(
        transaction: &Transaction,
        pricing_handling: PricingHandling,
        transaction_config: &TransactionConfig,
    ) -> Result<Self, InvalidTransaction> {
        match transaction {
            Transaction::Deploy(deploy) => MetaDeploy::from_deploy(
                deploy.clone(),
                pricing_handling,
                &transaction_config.transaction_v1_config,
            )
            .map(MetaTransaction::Deploy),
            Transaction::V1(v1) => MetaTransactionV1::from_transaction_v1(
                v1,
                &transaction_config.transaction_v1_config,
            )
            .map(MetaTransaction::V1),
        }
    }

    pub(crate) fn is_config_compliant(
        &self,
        chainspec: &Chainspec,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
    ) -> Result<(), InvalidTransaction> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .is_config_compliant(chainspec, timestamp_leeway, at)
                .map_err(InvalidTransaction::from),
            MetaTransaction::V1(v1) => v1
                .is_config_compliant(chainspec, timestamp_leeway, at)
                .map_err(InvalidTransaction::from),
        }
    }

    pub(crate) fn payload_hash(&self) -> Digest {
        match self {
            MetaTransaction::Deploy(meta_deploy) => *meta_deploy.deploy().body_hash(),
            MetaTransaction::V1(v1) => *v1.payload_hash(),
        }
    }

    pub(crate) fn to_session_input_data(&self) -> SessionInputData {
        let initiator_addr = self.initiator_addr();
        let is_standard_payment = self.is_standard_payment();
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                let deploy = meta_deploy.deploy();
                let data = SessionDataDeploy::new(
                    deploy.hash(),
                    deploy.session(),
                    initiator_addr,
                    self.signers().clone(),
                    is_standard_payment,
                );
                SessionInputData::DeploySessionData { data }
            }
            MetaTransaction::V1(v1) => {
                let data = SessionDataV1::new(
                    v1.args().as_named().expect("V1 wasm args should be named and validated at the transaction acceptor level"),
                    v1.target(),
                    v1.entry_point(),
                    v1.lane_id() == INSTALL_UPGRADE_LANE_ID,
                    v1.hash(),
                    v1.pricing_mode(),
                    initiator_addr,
                    self.signers().clone(),
                    is_standard_payment,
                );
                SessionInputData::SessionDataV1 { data }
            }
        }
    }

    /// Returns the `SessionInputData` for a payment code if present.
    pub(crate) fn to_payment_input_data(&self) -> SessionInputData {
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                let initiator_addr = meta_deploy.initiator_addr();
                let is_standard_payment = matches!(meta_deploy.deploy().payment(), ExecutableDeployItem::ModuleBytes { module_bytes, .. } if module_bytes.is_empty());
                let deploy = meta_deploy.deploy();
                let data = SessionDataDeploy::new(
                    deploy.hash(),
                    deploy.payment(),
                    initiator_addr,
                    self.signers().clone(),
                    is_standard_payment,
                );
                SessionInputData::DeploySessionData { data }
            }
            MetaTransaction::V1(v1) => {
                let initiator_addr = v1.initiator_addr();

                let is_standard_payment = if let PricingMode::PaymentLimited {
                    standard_payment,
                    ..
                } = v1.pricing_mode()
                {
                    *standard_payment
                } else {
                    true
                };

                // Under V1 transaction we don't have a separate payment code, and custom payment is
                // executed as session code with a phase set to Payment.
                let data = SessionDataV1::new(
                    v1.args().as_named().expect("V1 wasm args should be named and validated at the transaction acceptor level"),
                    v1.target(),
                    v1.entry_point(),
                    v1.lane_id() == INSTALL_UPGRADE_LANE_ID,
                    v1.hash(),
                    v1.pricing_mode(),
                    initiator_addr,
                    self.signers().clone(),
                    is_standard_payment,
                );
                SessionInputData::SessionDataV1 { data }
            }
        }
    }

    /// Size estimate.
    pub(crate) fn size_estimate(&self) -> usize {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.deploy().serialized_length(),
            MetaTransaction::V1(v1) => v1.serialized_length(),
        }
    }

    pub(crate) fn is_v1_wasm(&self) -> bool {
        match self {
            MetaTransaction::Deploy(_) => true,
            MetaTransaction::V1(v1) => v1.is_v1_wasm(),
        }
    }

    pub(crate) fn is_v2_wasm(&self) -> bool {
        match self {
            MetaTransaction::Deploy(_) => false,
            MetaTransaction::V1(v1) => v1.is_v2_wasm(),
        }
    }

    pub(crate) fn transferred_value(&self) -> Option<u64> {
        match self {
            MetaTransaction::Deploy(_) => None,
            MetaTransaction::V1(v1) => Some(v1.transferred_value()),
        }
    }

    pub(crate) fn target(&self) -> Option<TransactionTarget> {
        match self {
            MetaTransaction::Deploy(_) => None,
            MetaTransaction::V1(v1) => Some(v1.target().clone()),
        }
    }

    pub(crate) fn target_retrofit(&self) -> TransactionTarget {
        match self {
            MetaTransaction::Deploy(meta_deploy) => match meta_deploy.deploy().session() {
                ExecutableDeployItem::ModuleBytes {
                    module_bytes,
                    args: _,
                } => TransactionTarget::Session {
                    is_install_upgrade: true,
                    module_bytes: module_bytes.clone(),
                    runtime: TransactionRuntimeParams::VmCasperV1,
                },
                ExecutableDeployItem::StoredContractByHash {
                    hash,
                    entry_point: _,
                    args: _,
                } => TransactionTarget::Stored {
                    id: TransactionInvocationTarget::ByHash(hash.value()),
                    runtime: TransactionRuntimeParams::VmCasperV1,
                },
                ExecutableDeployItem::StoredContractByName {
                    name,
                    entry_point: _,
                    args: _,
                } => TransactionTarget::Stored {
                    id: TransactionInvocationTarget::ByName(name.clone()),
                    runtime: TransactionRuntimeParams::VmCasperV1,
                },
                ExecutableDeployItem::StoredVersionedContractByHash {
                    hash,
                    version,
                    entry_point: _entry_point,
                    args: _args,
                } => TransactionTarget::Stored {
                    id: TransactionInvocationTarget::ByPackageHash {
                        addr: PackageAddr::new(hash.value()),
                        version: *version,
                        protocol_version_major: None,
                    },
                    runtime: TransactionRuntimeParams::VmCasperV1,
                },
                ExecutableDeployItem::StoredVersionedContractByName {
                    name,
                    version,
                    entry_point: _entry_point,
                    args: _args,
                } => TransactionTarget::Stored {
                    id: TransactionInvocationTarget::ByPackageName {
                        name: name.to_string(),
                        version: *version,
                        protocol_version_major: None,
                    },
                    runtime: TransactionRuntimeParams::VmCasperV1,
                },
                ExecutableDeployItem::Transfer { args: _args } => TransactionTarget::Native,
            },
            MetaTransaction::V1(v1) => v1.target().clone(),
        }
    }

    pub(crate) fn transaction_args_retrofit(&self) -> TransactionArgs {
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                TransactionArgs::Named(meta_deploy.session().args().clone())
            }
            MetaTransaction::V1(meta_transaction_v1) => meta_transaction_v1.args().clone(),
        }
    }
}

impl Display for MetaTransaction {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            MetaTransaction::Deploy(meta_deploy) => Display::fmt(meta_deploy.deploy(), formatter),
            MetaTransaction::V1(txn) => Display::fmt(txn, formatter),
        }
    }
}

#[cfg(test)]
/// Calculates the laned based on properties of the transaction
pub(crate) fn calculate_transaction_lane_for_transaction(
    transaction: &Transaction,
    chainspec: &Chainspec,
) -> Result<u8, InvalidTransaction> {
    use casper_types::calculate_transaction_lane;

    match transaction {
        Transaction::Deploy(_) => {
            let meta = MetaTransaction::from_transaction(
                transaction,
                chainspec.core_config.pricing_handling,
                &chainspec.transaction_config,
            )?;
            Ok(meta.transaction_lane())
        }
        Transaction::V1(v1) => {
            let args_binary_len = v1
                .payload()
                .fields()
                .get(&ARGS_MAP_KEY)
                .map(|field| field.len())
                .unwrap_or(0);
            let target: TransactionTarget =
                v1.deserialize_field(TARGET_MAP_KEY).map_err(|error| {
                    InvalidTransaction::V1(InvalidTransactionV1::CouldNotDeserializeField { error })
                })?;
            let entry_point: TransactionEntryPoint =
                v1.deserialize_field(ENTRY_POINT_MAP_KEY).map_err(|error| {
                    InvalidTransaction::V1(InvalidTransactionV1::CouldNotDeserializeField { error })
                })?;
            let serialized_length = v1.serialized_length();
            let pricing_mode = v1.payload().pricing_mode();
            calculate_transaction_lane(
                &entry_point,
                &target,
                pricing_mode,
                &chainspec.transaction_config.transaction_v1_config,
                serialized_length as u64,
                args_binary_len as u64,
            )
            .map_err(InvalidTransaction::V1)
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use casper_types::{gens::legal_transaction_arb, TransactionLaneDefinition};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn construction_roundtrip(transaction in legal_transaction_arb()) {
            let mut transaction_config = TransactionConfig::default();
            transaction_config.transaction_v1_config.set_wasm_lanes(vec![
                TransactionLaneDefinition::new(
                     3,
                     u64::MAX/2,
                     10000,
                     u64::MAX/2,
                     10,
                ),
                TransactionLaneDefinition::new(
                     4,
                     u64::MAX,
                     10000,
                     u64::MAX,
                     10,
                ),
                ]);
            let maybe_transaction = MetaTransaction::from_transaction(&transaction, PricingHandling::PaymentLimited, &transaction_config);
            prop_assert!(maybe_transaction.is_ok(), "{:?}", maybe_transaction);
        }
    }
}
