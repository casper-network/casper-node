mod meta_deploy;
mod meta_evm;
mod meta_transaction_v1;
mod transaction_header;
use casper_execution_engine::engine_state::{SessionDataDeploy, SessionDataV1, SessionInputData};
#[cfg(test)]
use casper_types::InvalidTransactionV1;
use casper_types::{
    account::AccountHash, bytesrepr::ToBytes, Approval, Chainspec, Digest, EvmTransaction,
    ExecutableDeployItem, Gas, GasLimited, HashAddr, InitiatorAddr, InvalidTransaction, Phase,
    PricingHandling, PricingMode, TimeDiff, Timestamp, Transaction, TransactionArgs,
    TransactionConfig, TransactionEntryPoint, TransactionHash, TransactionTarget,
    INSTALL_UPGRADE_LANE_ID,
};
use core::fmt::{self, Debug, Display, Formatter};
use meta_deploy::MetaDeploy;
use meta_evm::MetaEvmTransaction;
pub(crate) use meta_transaction_v1::MetaTransactionV1;
use serde::Serialize;
use std::{borrow::Cow, collections::BTreeSet};
pub(crate) use transaction_header::*;

#[cfg(test)]
use super::fields_container::{ARGS_MAP_KEY, ENTRY_POINT_MAP_KEY, TARGET_MAP_KEY};

#[derive(Clone, Debug, Serialize)]
pub(crate) enum MetaTransaction {
    Deploy(MetaDeploy),
    Evm(MetaEvmTransaction),
    V1(MetaTransactionV1),
}

impl MetaTransaction {
    /// Returns the `TransactionHash` identifying this transaction.
    pub(crate) fn hash(&self) -> TransactionHash {
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                TransactionHash::from(*meta_deploy.deploy().hash())
            }
            MetaTransaction::Evm(evm) => evm.hash(),
            MetaTransaction::V1(txn) => TransactionHash::from(*txn.hash()),
        }
    }

    /// Timestamp.
    pub(crate) fn timestamp(&self) -> Timestamp {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.deploy().header().timestamp(),
            MetaTransaction::Evm(evm) => evm.timestamp(),
            MetaTransaction::V1(v1) => v1.timestamp(),
        }
    }

    /// Time to live.
    pub(crate) fn ttl(&self) -> TimeDiff {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.deploy().header().ttl(),
            MetaTransaction::Evm(evm) => evm.ttl(),
            MetaTransaction::V1(v1) => v1.ttl(),
        }
    }

    /// Returns the `Approval`s for this transaction.
    pub(crate) fn approvals(&self) -> BTreeSet<Approval> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.deploy().approvals().clone(),
            MetaTransaction::Evm(evm) => evm.approval().cloned().into_iter().collect(),
            MetaTransaction::V1(v1) => v1.approvals().clone(),
        }
    }

    /// Returns the Casper initiator address.
    pub(crate) fn initiator_addr(&self) -> &InitiatorAddr {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.initiator_addr(),
            MetaTransaction::Evm(evm) => evm.initiator_addr(),
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
            MetaTransaction::Evm(evm) => evm
                .approval()
                .into_iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
            MetaTransaction::V1(txn) => txn
                .approvals()
                .iter()
                .map(|approval| approval.signer().to_account_hash())
                .collect(),
        }
    }

    /// Returns `true` if `self` represents a native transfer deploy or a native V1 transaction.
    pub(crate) fn is_native(&self) -> bool {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.deploy().is_transfer(),
            MetaTransaction::Evm(_) => false,
            MetaTransaction::V1(v1_txn) => *v1_txn.target() == TransactionTarget::Native,
        }
    }

    pub(crate) fn is_wasm(&self) -> bool {
        match self {
            MetaTransaction::Deploy(meta_deploy) => !meta_deploy.deploy().is_transfer(),
            MetaTransaction::V1(v1_txn) => *v1_txn.target() != TransactionTarget::Native,
            MetaTransaction::Evm(_) => false,
        }
    }

    /// Should this transaction use standard payment processing?
    pub(crate) fn is_standard_payment(&self) -> bool {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .payment()
                .is_standard_payment(Phase::Payment),
            MetaTransaction::Evm(_) => true,
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
            MetaTransaction::Evm(_) => false,
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
            MetaTransaction::Evm(evm) => evm
                .approval()
                .into_iter()
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
    pub(crate) fn session_args(&self) -> Cow<'_, TransactionArgs> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => Cow::Owned(TransactionArgs::Named(
                meta_deploy.deploy().session().args().clone(),
            )),
            MetaTransaction::Evm(_) => {
                unreachable!("EVM transactions do not have Casper session args")
            }
            MetaTransaction::V1(transaction_v1) => Cow::Borrowed(transaction_v1.args()),
        }
    }

    /// The entry point.
    pub(crate) fn entry_point(&self) -> TransactionEntryPoint {
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                meta_deploy.deploy().session().entry_point_name().into()
            }
            MetaTransaction::Evm(_) => {
                unreachable!("EVM transactions do not have Casper entry points")
            }
            MetaTransaction::V1(transaction_v1) => transaction_v1.entry_point().clone(),
        }
    }

    /// The transaction lane.
    pub(crate) fn transaction_lane(&self) -> u8 {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.lane_id(),
            MetaTransaction::Evm(evm) => evm.lane_id(),
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
            MetaTransaction::Evm(evm) => Ok(evm.gas_price_tolerance()),
            MetaTransaction::V1(v1) => Ok(v1.gas_price_tolerance()),
        }
    }

    pub(crate) fn gas_limit(&self, chainspec: &Chainspec) -> Result<Gas, InvalidTransaction> {
        match self {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .gas_limit(chainspec)
                .map_err(InvalidTransaction::from),
            MetaTransaction::Evm(evm) => Ok(evm.gas_limit()),
            MetaTransaction::V1(v1) => v1.gas_limit(chainspec),
        }
    }

    /// Is the transaction the original transaction variant.
    pub(crate) fn is_deploy_transaction(&self) -> bool {
        match self {
            MetaTransaction::Deploy(_) => true,
            MetaTransaction::Evm(_) => false,
            MetaTransaction::V1(_) => false,
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
            MetaTransaction::Evm(_) => {}
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
            Transaction::Evm(evm) => {
                MetaEvmTransaction::from_evm_transaction(evm, transaction_config)
                    .map(MetaTransaction::Evm)
            }
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
            MetaTransaction::Evm(evm) => evm.is_config_compliant(chainspec).map_err(Into::into),
            MetaTransaction::V1(v1) => v1
                .is_config_compliant(chainspec, timestamp_leeway, at)
                .map_err(InvalidTransaction::from),
        }
    }

    pub(crate) fn payload_hash(&self) -> Digest {
        match self {
            MetaTransaction::Deploy(meta_deploy) => *meta_deploy.deploy().body_hash(),
            MetaTransaction::Evm(evm) => evm.payload_hash(),
            MetaTransaction::V1(v1) => *v1.payload_hash(),
        }
    }

    pub(crate) fn to_session_input_data(&self) -> SessionInputData<'_> {
        let is_standard_payment = self.is_standard_payment();
        match self {
            MetaTransaction::Deploy(meta_deploy) => {
                let deploy = meta_deploy.deploy();
                let initiator_addr = meta_deploy.initiator_addr();
                let data = SessionDataDeploy::new(
                    deploy.hash(),
                    deploy.session(),
                    initiator_addr,
                    self.signers().clone(),
                    is_standard_payment,
                );
                SessionInputData::DeploySessionData { data }
            }
            MetaTransaction::Evm(_) => {
                unreachable!("EVM transactions do not have Casper session input data")
            }
            MetaTransaction::V1(v1) => {
                let initiator_addr = v1.initiator_addr();
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
    pub(crate) fn to_payment_input_data(&self) -> SessionInputData<'_> {
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
            MetaTransaction::Evm(_) => {
                unreachable!("EVM transactions do not have Casper payment input data")
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
            MetaTransaction::Evm(evm) => evm.serialized_length(),
            MetaTransaction::V1(v1) => v1.serialized_length(),
        }
    }

    pub(crate) fn is_v1_wasm(&self) -> bool {
        match self {
            MetaTransaction::Deploy(_) => true,
            MetaTransaction::Evm(_) => false,
            MetaTransaction::V1(v1) => v1.is_v1_wasm(),
        }
    }

    pub(crate) fn is_v2_wasm(&self) -> bool {
        match self {
            MetaTransaction::Deploy(_) => false,
            MetaTransaction::Evm(_) => false,
            MetaTransaction::V1(v1) => v1.is_v2_wasm(),
        }
    }

    pub(crate) fn seed(&self) -> Option<[u8; 32]> {
        match self {
            MetaTransaction::Deploy(_) => None,
            MetaTransaction::Evm(_) => None,
            MetaTransaction::V1(v1) => v1.seed(),
        }
    }

    pub(crate) fn is_install_or_upgrade(&self) -> bool {
        match self {
            MetaTransaction::Deploy(_) => false,
            MetaTransaction::Evm(_) => false,
            MetaTransaction::V1(meta_transaction_v1) => {
                meta_transaction_v1.lane_id() == INSTALL_UPGRADE_LANE_ID
            }
        }
    }

    pub(crate) fn transferred_value(&self) -> Option<u64> {
        match self {
            MetaTransaction::Deploy(_) => None,
            MetaTransaction::Evm(_) => None,
            MetaTransaction::V1(v1) => Some(v1.transferred_value()),
        }
    }

    pub(crate) fn target(&self) -> Option<TransactionTarget> {
        match self {
            MetaTransaction::Deploy(_) => None,
            MetaTransaction::Evm(_) => None,
            MetaTransaction::V1(v1) => Some(v1.target().clone()),
        }
    }

    pub(crate) fn as_evm(&self) -> Option<&EvmTransaction> {
        match self {
            MetaTransaction::Evm(evm) => Some(evm.transaction()),
            _ => None,
        }
    }
}

impl Display for MetaTransaction {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            MetaTransaction::Deploy(meta_deploy) => Display::fmt(meta_deploy.deploy(), formatter),
            MetaTransaction::Evm(evm) => Display::fmt(evm, formatter),
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
        Transaction::Evm(_) => {
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
mod tests {
    use super::*;
    use alloy_consensus::{SignableTransaction, TxEip1559, TxEip7702, TxEnvelope, TxLegacy};
    use alloy_eips::{eip2718::Encodable2718, eip7702::Authorization as AlloyAuthorization};
    use alloy_primitives::{Address as AlloyAddress, Signature, TxKind, U256};
    use casper_types::{evm, EvmTransactionError, InitiatorAddr, TransactionLaneDefinition};

    const CHAIN_ID: u64 = 7;
    const BASE_FEE: u64 = 1_000_000;
    const EVM_LANE: u8 = 4;

    #[test]
    fn evm_from_transaction_exposes_metadata() {
        let chainspec = chainspec();
        let evm_transaction = legacy_transaction(Some(CHAIN_ID), BASE_FEE.into(), 21_000);
        let transaction = Transaction::Evm(evm_transaction.clone());
        let meta = MetaTransaction::from_transaction(
            &transaction,
            chainspec.core_config.pricing_handling,
            &chainspec.transaction_config,
        )
        .expect("EVM transaction metadata should be created");

        assert_eq!(meta.hash(), transaction.hash());
        assert_eq!(meta.timestamp(), evm_transaction.timestamp());
        assert_eq!(meta.ttl(), evm_transaction.ttl());
        assert_eq!(
            meta.approvals(),
            evm_transaction.approval().cloned().into_iter().collect()
        );
        assert_eq!(meta.initiator_addr(), evm_transaction.initiator_addr());
        assert_eq!(meta.transaction_lane(), EVM_LANE);
        assert_eq!(meta.gas_limit(&chainspec).unwrap(), Gas::new(21_000));
        assert_eq!(meta.gas_price_tolerance().unwrap(), u8::MAX);
        assert_eq!(meta.size_estimate(), evm_transaction.serialized_length());
        assert!(meta.is_standard_payment());
        assert!(!meta.is_custom_payment());
        assert!(!meta.is_v1_wasm());
        assert!(!meta.is_v2_wasm());
        assert!(meta.seed().is_none());
        meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero())
            .expect("valid EVM transaction should be config compliant");
    }

    #[test]
    fn evm_transaction_header_keeps_initiator_addr() {
        let evm_transaction = legacy_transaction(Some(CHAIN_ID), BASE_FEE.into(), 21_000);
        let expected_signer = evm_transaction
            .signer()
            .expect("signed EVM transaction should have an approval signer")
            .clone();
        let expected_initiator_addr = InitiatorAddr::AccountHash(expected_signer.to_account_hash());

        assert_eq!(
            Transaction::Evm(evm_transaction.clone()).initiator_addr(),
            expected_initiator_addr
        );

        let header = TransactionHeader::from(&evm_transaction);
        let TransactionHeader::Evm(metadata) = header else {
            panic!("expected EVM transaction header");
        };
        assert_eq!(metadata.initiator_addr(), &expected_initiator_addr);
    }

    #[test]
    fn evm_from_transaction_requires_lane() {
        let mut chainspec = chainspec();
        chainspec
            .transaction_config
            .transaction_v1_config
            .set_wasm_lanes(vec![]);
        let transaction =
            Transaction::Evm(legacy_transaction(Some(CHAIN_ID), BASE_FEE.into(), 21_000));
        let error = MetaTransaction::from_transaction(
            &transaction,
            chainspec.core_config.pricing_handling,
            &chainspec.transaction_config,
        )
        .expect_err("EVM transaction should need a lane");
        assert!(matches!(
            error,
            InvalidTransaction::Evm(EvmTransactionError::MissingTransactionLane)
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_disabled_evm() {
        let mut chainspec = chainspec();
        chainspec.evm_config.enabled = false;
        let meta = evm_meta(
            &chainspec,
            legacy_transaction(Some(CHAIN_ID), BASE_FEE.into(), 21_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::Disabled))
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_missing_chain_id() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            legacy_transaction(None, BASE_FEE.into(), 21_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::MissingChainId))
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_mismatched_chain_id() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            legacy_transaction(Some(CHAIN_ID + 1), BASE_FEE.into(), 21_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::ChainIdMismatch {
                expected: CHAIN_ID,
                actual
            })) if actual == CHAIN_ID + 1
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_gas_price_below_base_fee() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            legacy_transaction(Some(CHAIN_ID), (BASE_FEE - 1).into(), 21_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::GasPriceBelowBaseFee {
                gas_price,
                base_fee
            })) if gas_price == u128::from(BASE_FEE - 1) && base_fee == u128::from(BASE_FEE)
        ));
    }

    #[test]
    fn evm_config_compliance_accepts_unsigned_call() {
        let chainspec = chainspec();
        let meta = evm_meta(&chainspec, unsigned_call(CHAIN_ID, BASE_FEE.into(), 21_000));
        meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero())
            .expect("unsigned EVM call should be config compliant");
    }

    #[test]
    fn evm_config_compliance_rejects_unsigned_call_mismatched_chain_id() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            unsigned_call(CHAIN_ID + 1, BASE_FEE.into(), 21_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::ChainIdMismatch {
                expected: CHAIN_ID,
                actual
            })) if actual == CHAIN_ID + 1
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_unsigned_call_gas_price_below_base_fee() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            unsigned_call(CHAIN_ID, u128::from(BASE_FEE - 1), 21_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::GasPriceBelowBaseFee {
                gas_price,
                base_fee
            })) if gas_price == u128::from(BASE_FEE - 1) && base_fee == u128::from(BASE_FEE)
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_max_fee_below_base_fee() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            eip1559_transaction(u128::from(BASE_FEE - 1), 0, 60_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::MaxFeePerGasBelowBaseFee {
                max_fee_per_gas,
                base_fee
            })) if max_fee_per_gas == u128::from(BASE_FEE - 1) && base_fee == u128::from(BASE_FEE)
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_non_zero_priority_fee() {
        let chainspec = chainspec();
        let meta = evm_meta(&chainspec, eip1559_transaction(BASE_FEE.into(), 1, 60_000));
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(
                EvmTransactionError::NonZeroMaxPriorityFeePerGas {
                    max_priority_fee_per_gas: 1
                }
            ))
        ));
    }

    #[test]
    fn evm_config_compliance_accepts_eip7702() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            eip7702_transaction(CHAIN_ID, BASE_FEE.into(), 0, 60_000),
        );
        meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero())
            .expect("valid EIP-7702 transaction should be config compliant");
    }

    #[test]
    fn evm_config_compliance_rejects_eip7702_mismatched_chain_id() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            eip7702_transaction(CHAIN_ID + 1, BASE_FEE.into(), 0, 60_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::ChainIdMismatch {
                expected: CHAIN_ID,
                actual
            })) if actual == CHAIN_ID + 1
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_eip7702_max_fee_below_base_fee() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            eip7702_transaction(CHAIN_ID, u128::from(BASE_FEE - 1), 0, 60_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::MaxFeePerGasBelowBaseFee {
                max_fee_per_gas,
                base_fee
            })) if max_fee_per_gas == u128::from(BASE_FEE - 1) && base_fee == u128::from(BASE_FEE)
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_eip7702_non_zero_priority_fee() {
        let chainspec = chainspec();
        let meta = evm_meta(
            &chainspec,
            eip7702_transaction(CHAIN_ID, BASE_FEE.into(), 1, 60_000),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(
                EvmTransactionError::NonZeroMaxPriorityFeePerGas {
                    max_priority_fee_per_gas: 1
                }
            ))
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_gas_limit_above_block_limit() {
        let chainspec = chainspec();
        let gas_limit = chainspec.evm_config.block_gas_limit + 1;
        let meta = evm_meta(
            &chainspec,
            legacy_transaction(Some(CHAIN_ID), BASE_FEE.into(), gas_limit),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::GasLimitExceedsBlockGasLimit {
                gas_limit: actual_gas_limit,
                block_gas_limit
            })) if actual_gas_limit == gas_limit && block_gas_limit == chainspec.evm_config.block_gas_limit
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_eip7702_gas_limit_above_block_limit() {
        let chainspec = chainspec();
        let gas_limit = chainspec.evm_config.block_gas_limit + 1;
        let meta = evm_meta(
            &chainspec,
            eip7702_transaction(CHAIN_ID, BASE_FEE.into(), 0, gas_limit),
        );
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(EvmTransactionError::GasLimitExceedsBlockGasLimit {
                gas_limit: actual_gas_limit,
                block_gas_limit
            })) if actual_gas_limit == gas_limit && block_gas_limit == chainspec.evm_config.block_gas_limit
        ));
    }

    #[test]
    fn evm_config_compliance_rejects_invalid_approval() {
        let chainspec = chainspec();
        let evm_transaction =
            legacy_transaction(Some(CHAIN_ID), BASE_FEE.into(), 21_000).with_evm_approval(None);
        let meta = evm_meta(&chainspec, evm_transaction);
        assert!(matches!(
            meta.is_config_compliant(&chainspec, TimeDiff::from_seconds(0), Timestamp::zero()),
            Err(InvalidTransaction::Evm(
                EvmTransactionError::MissingApproval
            ))
        ));
    }

    fn chainspec() -> Chainspec {
        let mut chainspec = Chainspec::default();
        chainspec.evm_config.enabled = true;
        chainspec.evm_config.chain_id = CHAIN_ID;
        chainspec.evm_config.base_fee = BASE_FEE;
        chainspec.evm_config.block_gas_limit = 30_000_000;
        chainspec
            .transaction_config
            .transaction_v1_config
            .set_wasm_lanes(vec![TransactionLaneDefinition::new(
                EVM_LANE,
                u64::MAX,
                10_000,
                u64::MAX,
                10,
            )]);
        chainspec
    }

    fn evm_meta(chainspec: &Chainspec, evm_transaction: EvmTransaction) -> MetaTransaction {
        MetaTransaction::from_transaction(
            &Transaction::Evm(evm_transaction),
            chainspec.core_config.pricing_handling,
            &chainspec.transaction_config,
        )
        .expect("EVM transaction metadata should be created")
    }

    fn unsigned_call(chain_id: u64, gas_price: u128, gas_limit: u64) -> EvmTransaction {
        EvmTransaction::new_unsigned_call(
            Timestamp::zero(),
            TimeDiff::from_seconds(60),
            test_initiator_addr(),
            chain_id,
            evm::Address::new([1u8; 20]),
            Some(evm::Address::new([2u8; 20])),
            casper_types::U256::zero(),
            Default::default(),
            gas_limit,
            gas_price,
        )
    }

    fn test_initiator_addr() -> InitiatorAddr {
        InitiatorAddr::AccountHash(AccountHash::new([8; 32]))
    }

    fn legacy_transaction(
        chain_id: Option<u64>,
        gas_price: u128,
        gas_limit: u64,
    ) -> EvmTransaction {
        // Ethereum legacy transactions are the original, untyped transaction
        // envelope. With EIP-155 replay protection they include a chain ID,
        // but they still use a single fixed `gas_price` instead of separate
        // base-fee and priority-fee fields.
        let tx = TxLegacy {
            chain_id,
            nonce: 0,
            gas_price,
            gas_limit,
            to: TxKind::Call(AlloyAddress::from([1u8; 20])),
            value: U256::ZERO,
            input: Default::default(),
        };
        signed_transaction(tx.into_signed(Signature::test_signature()).into())
    }

    fn eip1559_transaction(
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
        gas_limit: u64,
    ) -> EvmTransaction {
        // EIP-1559 transactions are typed dynamic-fee transactions. Casper
        // currently accepts this envelope for tooling compatibility, but
        // requires `max_priority_fee_per_gas == 0` because transactions are
        // not packed by priority fee.
        let tx = TxEip1559 {
            chain_id: CHAIN_ID,
            nonce: 0,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            to: TxKind::Call(AlloyAddress::from([1u8; 20])),
            value: U256::ZERO,
            access_list: Default::default(),
            input: Default::default(),
        };
        signed_transaction(tx.into_signed(Signature::test_signature()).into())
    }

    fn eip7702_transaction(
        chain_id: u64,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
        gas_limit: u64,
    ) -> EvmTransaction {
        let authorization = AlloyAuthorization {
            chain_id: U256::from(chain_id),
            address: AlloyAddress::from([2u8; 20]),
            nonce: 0,
        }
        .into_signed(Signature::test_signature());
        let tx = TxEip7702 {
            chain_id,
            nonce: 0,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            to: AlloyAddress::from([1u8; 20]),
            value: U256::ZERO,
            access_list: Default::default(),
            authorization_list: vec![authorization],
            input: Default::default(),
        };
        signed_transaction(tx.into_signed(Signature::test_signature()).into())
    }

    fn signed_transaction(envelope: TxEnvelope) -> EvmTransaction {
        EvmTransaction::from_signed_rlp(
            envelope.encoded_2718(),
            Timestamp::zero(),
            TimeDiff::from_seconds(60),
        )
        .expect("EVM transaction should decode")
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
                TransactionLaneDefinition::new(3, u64::MAX / 2, 10000, u64::MAX / 2, 10),
                TransactionLaneDefinition::new(4, u64::MAX, 10000, u64::MAX, 10),
                ]);
            let maybe_transaction = MetaTransaction::from_transaction(&transaction, PricingHandling::PaymentLimited, &transaction_config);
            prop_assert!(maybe_transaction.is_ok(), "{:?}", maybe_transaction);
        }
    }
}
