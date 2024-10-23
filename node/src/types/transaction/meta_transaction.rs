mod meta_transaction_v1;
mod tranasction_lane;
mod transaction_header;
pub(crate) use transaction_header::*;

use casper_execution_engine::engine_state::{SessionDataDeploy, SessionDataV1, SessionInputData};
use casper_types::{
    account::AccountHash, bytesrepr::ToBytes, Approval, Chainspec, Deploy, Digest,
    ExecutableDeployItem, Gas, GasLimited, HashAddr, InitiatorAddr, InvalidTransaction, Phase,
    PricingMode, RuntimeArgs, TimeDiff, Timestamp, Transaction, TransactionConfig,
    TransactionEntryPoint, TransactionHash, TransactionTarget, INSTALL_UPGRADE_LANE_ID,
    LARGE_WASM_LANE_ID, MINT_LANE_ID,
};
use core::fmt::{self, Debug, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
pub(crate) use meta_transaction_v1::MetaTransactionV1;
use serde::Serialize;
use std::collections::BTreeSet;

#[cfg_attr(feature = "datasize", derive(DataSize))]
#[derive(Clone, Debug, Serialize)]
pub(crate) enum MetaTransaction {
    Deploy(Deploy),
    V1(MetaTransactionV1),
}

impl MetaTransaction {
    /// Returns the `TransactionHash` identifying this transaction.
    pub fn hash(&self) -> TransactionHash {
        match self {
            MetaTransaction::Deploy(deploy) => TransactionHash::from(*deploy.hash()),
            MetaTransaction::V1(txn) => TransactionHash::from(*txn.hash()),
        }
    }

    /// Timestamp.
    pub fn timestamp(&self) -> Timestamp {
        match self {
            MetaTransaction::Deploy(deploy) => deploy.header().timestamp(),
            MetaTransaction::V1(v1) => v1.timestamp(),
        }
    }

    /// Time to live.
    pub fn ttl(&self) -> TimeDiff {
        match self {
            MetaTransaction::Deploy(deploy) => deploy.header().ttl(),
            MetaTransaction::V1(v1) => v1.ttl(),
        }
    }

    /// Returns the `Approval`s for this transaction.
    pub fn approvals(&self) -> BTreeSet<Approval> {
        match self {
            MetaTransaction::Deploy(deploy) => deploy.approvals().clone(),
            MetaTransaction::V1(v1) => v1.approvals().clone(),
        }
    }

    /// Returns the address of the initiator of the transaction.
    pub fn initiator_addr(&self) -> InitiatorAddr {
        match self {
            MetaTransaction::Deploy(deploy) => InitiatorAddr::PublicKey(deploy.account().clone()),
            MetaTransaction::V1(txn) => txn.initiator_addr().clone(),
        }
    }

    /// Returns the set of account hashes corresponding to the public keys of the approvals.
    pub fn signers(&self) -> BTreeSet<AccountHash> {
        match self {
            MetaTransaction::Deploy(deploy) => deploy
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

    /// Returns `true` if `self` represents a native transfer deploy or a native V1 transaction.
    pub fn is_native(&self) -> bool {
        match self {
            MetaTransaction::Deploy(deploy) => deploy.is_transfer(),
            MetaTransaction::V1(v1_txn) => *v1_txn.target() == TransactionTarget::Native,
        }
    }

    /// Should this transaction use standard payment processing?
    pub fn is_standard_payment(&self) -> bool {
        match self {
            MetaTransaction::Deploy(deploy) => deploy.payment().is_standard_payment(Phase::Payment),
            MetaTransaction::V1(v1) => {
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

    /// Authorization keys.
    pub fn authorization_keys(&self) -> BTreeSet<AccountHash> {
        match self {
            MetaTransaction::Deploy(deploy) => deploy
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
    pub fn session_args(&self) -> &RuntimeArgs {
        match self {
            MetaTransaction::Deploy(deploy) => deploy.session().args(),
            MetaTransaction::V1(transaction_v1) => transaction_v1.args(),
        }
    }

    /// The entry point.
    pub fn entry_point(&self) -> TransactionEntryPoint {
        match self {
            MetaTransaction::Deploy(deploy) => deploy.session().entry_point_name().into(),
            MetaTransaction::V1(transaction_v1) => transaction_v1.entry_point().clone(),
        }
    }

    /// The transaction lane.
    pub fn transaction_lane(&self) -> u8 {
        match self {
            MetaTransaction::Deploy(deploy) => {
                if deploy.is_transfer() {
                    MINT_LANE_ID
                } else {
                    LARGE_WASM_LANE_ID
                }
            }
            MetaTransaction::V1(v1) => v1.transaction_lane(),
        }
    }

    /// Returns the gas price tolerance.
    pub fn gas_price_tolerance(&self) -> Result<u8, InvalidTransaction> {
        match self {
            MetaTransaction::Deploy(deploy) => deploy
                .gas_price_tolerance()
                .map_err(InvalidTransaction::from),
            MetaTransaction::V1(v1) => Ok(v1.gas_price_tolerance()),
        }
    }

    pub fn gas_limit(&self, chainspec: &Chainspec) -> Result<Gas, InvalidTransaction> {
        match self {
            MetaTransaction::Deploy(deploy) => deploy
                .gas_limit(chainspec)
                .map_err(InvalidTransaction::from),
            MetaTransaction::V1(v1) => v1.gas_limit(chainspec),
        }
    }

    /// Is the transaction the original transaction variant.
    pub fn is_deploy_transaction(&self) -> bool {
        match self {
            MetaTransaction::Deploy(_) => true,
            MetaTransaction::V1(_) => false,
        }
    }

    /// Does this transaction provide the hash addr for a specific contract to invoke directly?
    pub fn is_contract_by_hash_invocation(&self) -> bool {
        self.contract_direct_address().is_some()
    }

    /// Returns a `hash_addr` for a targeted contract, if known.
    pub fn contract_direct_address(&self) -> Option<(HashAddr, String)> {
        match self {
            MetaTransaction::Deploy(deploy) => {
                if let ExecutableDeployItem::StoredContractByHash {
                    hash, entry_point, ..
                } = deploy.session()
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

    pub fn from(
        transaction: &Transaction,
        transaction_config: &TransactionConfig,
    ) -> Result<Self, InvalidTransaction> {
        match transaction {
            Transaction::Deploy(deploy) => Ok(MetaTransaction::Deploy(deploy.clone())),
            Transaction::V1(v1) => {
                MetaTransactionV1::from(v1, transaction_config).map(MetaTransaction::V1)
            }
        }
    }

    pub fn is_config_compliant(
        &self,
        chainspec: &Chainspec,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
    ) -> Result<(), InvalidTransaction> {
        match self {
            MetaTransaction::Deploy(deploy) => deploy
                .is_config_compliant(chainspec, timestamp_leeway, at)
                .map_err(InvalidTransaction::from),
            MetaTransaction::V1(v1) => v1
                .is_config_compliant(chainspec, timestamp_leeway, at)
                .map_err(InvalidTransaction::from),
        }
    }

    pub fn payload_hash(&self) -> Digest {
        match self {
            MetaTransaction::Deploy(deploy) => *deploy.body_hash(),
            MetaTransaction::V1(v1) => *v1.payload_hash(),
        }
    }

    pub fn to_session_input_data(&self) -> SessionInputData {
        let initiator_addr = self.initiator_addr();
        let is_standard_payment = self.is_standard_payment();
        match self {
            MetaTransaction::Deploy(deploy) => {
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
                    v1.args(),
                    v1.target(),
                    v1.entry_point(),
                    v1.transaction_lane() == INSTALL_UPGRADE_LANE_ID,
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
    pub fn size_estimate(&self) -> usize {
        match self {
            MetaTransaction::Deploy(deploy) => deploy.serialized_length(),
            MetaTransaction::V1(v1) => v1.serialized_length(),
        }
    }
}

impl Display for MetaTransaction {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            MetaTransaction::Deploy(deploy) => Display::fmt(deploy, formatter),
            MetaTransaction::V1(txn) => Display::fmt(txn, formatter),
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use casper_types::gens::legal_transaction_arb;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn construction_roundtrip(transaction in legal_transaction_arb()) {
            let maybe_transaction = MetaTransaction::from(&transaction, &TransactionConfig::default());
            assert!(maybe_transaction.is_ok());
        }
    }
}
