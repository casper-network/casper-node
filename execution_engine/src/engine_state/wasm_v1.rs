use std::{collections::BTreeSet, convert::TryFrom};

use serde::Serialize;
use thiserror::Error;

use casper_storage::data_access_layer::TransferResult;
use casper_types::{
    account::AccountHash, bytesrepr::Bytes, contract_messages::Messages, execution::Effects,
    runtime_args, system::mint::ARG_AMOUNT, BlockTime, DeployHash, Digest, ExecutableDeployItem,
    Gas, InitiatorAddr, Phase, PricingMode, RuntimeArgs, Transaction, TransactionEntryPoint,
    TransactionHash, TransactionInvocationTarget, TransactionSessionKind, TransactionTarget,
    TransactionV1, Transfer, U512,
};

use crate::engine_state::{DeployItem, Error as EngineError, ExecutionResult};

const DEFAULT_ENTRY_POINT: &str = "call";

/// Error returned if constructing a new [`WasmV1Request`] fails.
#[derive(Clone, Eq, PartialEq, Error, Serialize, Debug)]
pub enum InvalidRequest {
    /// Missing custom payment.
    #[error("custom payment not found for {0}")]
    CustomPaymentNotFound(TransactionHash),
    /// Unexpected variant.
    #[error("unexpected variant for {0} attempting {1}")]
    UnexpectedVariant(TransactionHash, String),
    /// Unsupported mode.
    #[error("unsupported mode for {0} attempting {1}")]
    UnsupportedMode(TransactionHash, String),
    /// Invalid entry point.
    #[error("invalid entry point for {0} attempting {1}")]
    InvalidEntryPoint(TransactionHash, String),
    /// Invalid target.
    #[error("invalid target for {0} attempting {1}")]
    InvalidTarget(TransactionHash, String),
}

/// The item to be executed.
#[derive(Debug)]
pub enum ExecutableItem<'a> {
    /// A stored entity or package.
    Stored(TransactionInvocationTarget),
    /// Compiled Wasm from a transaction >= V1 as byte code.
    SessionModuleBytes {
        /// The kind of session.
        kind: TransactionSessionKind,
        /// The compiled Wasm.
        module_bytes: &'a Bytes,
    },
    /// Compiled Wasm from a deploy as byte code.
    DeploySessionModuleBytes(&'a Bytes),
    /// Module bytes to be used as custom payment.
    CustomPayment(&'a Bytes),
    /// Standard payment.
    StandardPayment,
}

impl<'a> ExecutableItem<'a> {
    pub(super) fn phase(&self) -> Phase {
        match self {
            ExecutableItem::Stored(_)
            | ExecutableItem::SessionModuleBytes { .. }
            | ExecutableItem::DeploySessionModuleBytes(_) => Phase::Session,
            ExecutableItem::CustomPayment(_) | ExecutableItem::StandardPayment => Phase::Payment,
        }
    }
}

/// A request to execute the given Wasm on the V1 runtime.
#[derive(Debug)]
pub struct WasmV1Request<'a> {
    /// State root hash of the global state in which the transaction will be executed.
    pub state_hash: Digest,
    /// Block time represented as a unix timestamp.
    pub block_time: BlockTime,
    /// The hash identifying the transaction.
    pub transaction_hash: TransactionHash,
    /// The number of Motes per unit of Gas to be paid for execution.
    pub gas_limit: Gas,
    /// The transaction's initiator.
    pub initiator_addr: InitiatorAddr,
    /// The executable item.
    pub executable_item: ExecutableItem<'a>,
    /// The entry point to call when executing.
    pub entry_point: String,
    /// The runtime args.
    pub args: RuntimeArgs,
    /// The account hashes of the signers of the transaction.
    pub authorization_keys: BTreeSet<AccountHash>,
}

impl<'a> WasmV1Request<'a> {
    /// Creates a new request from a transaction for use as the session code.
    pub fn new_session(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        txn: &'a Transaction,
    ) -> Result<Self, InvalidRequest> {
        let transaction_hash = txn.hash();
        let initiator_addr = txn.initiator_addr();
        let authorization_keys = txn.signers();

        let session_info = match txn {
            Transaction::Deploy(deploy) => {
                SessionInfo::try_from((deploy.session(), deploy.hash()))?
            }
            Transaction::V1(v1_txn) => SessionInfo::try_from(v1_txn)?,
        };

        Ok(Self {
            state_hash,
            block_time,
            transaction_hash,
            gas_limit,
            initiator_addr,
            executable_item: session_info.session,
            entry_point: session_info.entry_point,
            args: session_info.args,
            authorization_keys,
        })
    }

    /// Creates a new request from a transaction for use as custom payment.
    pub fn new_custom_payment(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        txn: &'a Transaction,
    ) -> Result<Self, InvalidRequest> {
        let transaction_hash = txn.hash();
        let initiator_addr = txn.initiator_addr();
        let authorization_keys = txn.signers();

        let payment_info = match txn {
            Transaction::Deploy(deploy) => {
                PaymentInfo::try_from((deploy.payment(), deploy.hash()))?
            }
            Transaction::V1(v1_txn) => PaymentInfo::try_from(v1_txn)?,
        };

        Ok(Self {
            state_hash,
            block_time,
            transaction_hash,
            gas_limit,
            initiator_addr,
            executable_item: payment_info.payment,
            entry_point: DEFAULT_ENTRY_POINT.to_string(),
            args: payment_info.args,
            authorization_keys,
        })
    }

    /// Creates a new request from a deploy item for use as the session code.
    //
    // TODO - deprecate?
    pub fn new_session_from_deploy_item(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        DeployItem {
            ref address,
            ref session,
            ref authorization_keys,
            ref deploy_hash,
            ..
        }: &'a DeployItem,
    ) -> Result<Self, InvalidRequest> {
        let session_info = SessionInfo::try_from((session, deploy_hash))?;
        Ok(Self {
            state_hash,
            block_time,
            transaction_hash: TransactionHash::Deploy(*deploy_hash),
            gas_limit,
            initiator_addr: InitiatorAddr::AccountHash(*address),
            executable_item: session_info.session,
            entry_point: session_info.entry_point,
            args: session_info.args,
            authorization_keys: authorization_keys.clone(),
        })
    }

    /// Creates a new request from a deploy item for use as custom payment.
    //
    // TODO - deprecate?
    pub fn new_custom_payment_from_deploy_item(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        DeployItem {
            ref address,
            ref payment,
            ref authorization_keys,
            ref deploy_hash,
            ..
        }: &'a DeployItem,
    ) -> Result<Self, InvalidRequest> {
        let payment_info = PaymentInfo::try_from((payment, deploy_hash))?;
        Ok(Self {
            state_hash,
            block_time,
            transaction_hash: TransactionHash::Deploy(*deploy_hash),
            gas_limit,
            initiator_addr: InitiatorAddr::AccountHash(*address),
            executable_item: payment_info.payment,
            entry_point: DEFAULT_ENTRY_POINT.to_string(),
            args: payment_info.args,
            authorization_keys: authorization_keys.clone(),
        })
    }
}

/// Wasm v1 result.
#[derive(Clone, Debug)]
pub struct WasmV1Result {
    /// List of transfers that happened during execution.
    transfers: Vec<Transfer>,
    /// Gas limit.
    limit: Gas,
    /// Gas consumed.
    consumed: Gas,
    /// Execution effects.
    effects: Effects,
    /// Messages emitted during execution.
    messages: Messages,
    /// Did the wasm execute successfully?
    error: Option<EngineError>,
}

impl WasmV1Result {
    /// Error, if any.
    pub fn error(&self) -> Option<&EngineError> {
        self.error.as_ref()
    }

    /// List of transfers that happened during execution.
    pub fn transfers(&self) -> &Vec<Transfer> {
        &self.transfers
    }

    /// Gas limit.
    pub fn limit(&self) -> Gas {
        self.limit
    }

    /// Gas consumed.
    pub fn consumed(&self) -> Gas {
        self.consumed
    }

    /// Execution effects.
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    /// Messages emitted during execution.
    pub fn messages(&self) -> &Messages {
        &self.messages
    }

    /// Root not found.
    pub fn root_not_found(gas_limit: Gas, state_hash: Digest) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(EngineError::RootNotFound(state_hash)),
        }
    }

    /// Precondition failure.
    pub fn precondition_failure(gas_limit: Gas, error: EngineError) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(error),
        }
    }

    /// Failed to transform transaction into an executable item.
    pub fn invalid_executable_item(gas_limit: Gas, error: InvalidRequest) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(EngineError::InvalidExecutableItem(error)),
        }
    }

    /// Returns `true` if this is a precondition failure.
    ///
    /// Precondition variant is further described as an execution failure which does not have any
    /// effects, and has a gas cost of 0.
    pub fn has_precondition_failure(&self) -> bool {
        self.error.is_some() && self.consumed == Gas::zero() && self.effects.is_empty()
    }

    /// From an execution result.
    pub fn from_execution_result(gas_limit: Gas, execution_result: ExecutionResult) -> Self {
        match execution_result {
            ExecutionResult::Failure {
                error,
                transfers,
                gas,
                effects,
                messages,
            } => WasmV1Result {
                error: Some(error),
                transfers,
                effects,
                messages,
                limit: gas_limit,
                consumed: gas,
            },
            ExecutionResult::Success {
                transfers,
                gas,
                effects,
                messages,
            } => WasmV1Result {
                transfers,
                effects,
                messages,
                limit: gas_limit,
                consumed: gas,
                error: None,
            },
        }
    }

    /// Converts a transfer result to an execution result.
    pub fn from_transfer_result(transfer_result: TransferResult, consumed: Gas) -> Option<Self> {
        match transfer_result {
            TransferResult::RootNotFound => None,
            TransferResult::Success { transfers, effects } => {
                Some(WasmV1Result {
                    transfers,
                    limit: consumed, // TODO - check this is legit.
                    consumed,
                    effects,
                    messages: Messages::default(),
                    error: None,
                })
            }
            TransferResult::Failure(te) => {
                Some(WasmV1Result {
                    transfers: vec![],
                    limit: consumed, // TODO - check this is legit.
                    consumed,
                    effects: Effects::default(), // currently not returning effects on failure
                    messages: Messages::default(),
                    error: Some(EngineError::Transfer(te)),
                })
            }
        }
    }
}

/// Helper struct to carry the appropriate info for converting an `ExecutableDeployItem` or a
/// `TransactionV1` into the corresponding fields of a `WasmV1Request` for execution as session
/// code.
struct SessionInfo<'a> {
    session: ExecutableItem<'a>,
    entry_point: String,
    args: RuntimeArgs,
}

impl<'a> TryFrom<(&'a ExecutableDeployItem, &'a DeployHash)> for SessionInfo<'a> {
    type Error = InvalidRequest;

    fn try_from(
        (session_item, deploy_hash): (&'a ExecutableDeployItem, &'a DeployHash),
    ) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::Deploy(*deploy_hash);
        let session: ExecutableItem<'a>;
        let session_entry_point: String;
        let session_args: RuntimeArgs;
        match session_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                session = ExecutableItem::DeploySessionModuleBytes(module_bytes);
                session_entry_point = DEFAULT_ENTRY_POINT.to_string();
                session_args = args.clone();
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                session = ExecutableItem::Stored(
                    TransactionInvocationTarget::new_invocable_entity(*hash),
                );
                session_entry_point = entry_point.clone();
                session_args = args.clone();
            }
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                session = ExecutableItem::Stored(
                    TransactionInvocationTarget::new_invocable_entity_alias(name.clone()),
                );
                session_entry_point = entry_point.clone();
                session_args = args.clone();
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => {
                session = ExecutableItem::Stored(TransactionInvocationTarget::new_package(
                    *hash, *version,
                ));
                session_entry_point = entry_point.clone();
                session_args = args.clone();
            }
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => {
                session = ExecutableItem::Stored(TransactionInvocationTarget::new_package_alias(
                    name.clone(),
                    *version,
                ));
                session_entry_point = entry_point.clone();
                session_args = args.clone();
            }
            ExecutableDeployItem::Transfer { .. } => {
                return Err(InvalidRequest::UnsupportedMode(
                    transaction_hash,
                    session_item.to_string(),
                ));
            }
        }

        Ok(SessionInfo {
            session,
            entry_point: session_entry_point,
            args: session_args,
        })
    }
}

impl<'a> TryFrom<&'a TransactionV1> for SessionInfo<'a> {
    type Error = InvalidRequest;

    fn try_from(v1_txn: &'a TransactionV1) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::V1(*v1_txn.hash());
        let args = v1_txn.args().clone();
        let session = match v1_txn.target() {
            TransactionTarget::Native => {
                return Err(InvalidRequest::InvalidTarget(
                    transaction_hash,
                    v1_txn.target().to_string(),
                ));
            }
            TransactionTarget::Stored { id, .. } => ExecutableItem::Stored(id.clone()),
            TransactionTarget::Session {
                kind, module_bytes, ..
            } => ExecutableItem::SessionModuleBytes {
                kind: *kind,
                module_bytes,
            },
        };

        let TransactionEntryPoint::Custom(entry_point) = v1_txn.entry_point() else {
            return Err(InvalidRequest::InvalidEntryPoint(transaction_hash, v1_txn.entry_point().to_string()));
        };

        Ok(SessionInfo {
            session,
            entry_point: entry_point.clone(),
            args,
        })
    }
}

/// Helper struct to carry the appropriate info for converting an `ExecutableDeployItem` or a
/// `TransactionV1` into the corresponding fields of a `WasmV1Request` for execution as custom
/// payment.
struct PaymentInfo<'a> {
    payment: ExecutableItem<'a>,
    args: RuntimeArgs,
}

impl<'a> TryFrom<(&'a ExecutableDeployItem, &'a DeployHash)> for PaymentInfo<'a> {
    type Error = InvalidRequest;

    fn try_from(
        (payment_item, deploy_hash): (&'a ExecutableDeployItem, &'a DeployHash),
    ) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::Deploy(*deploy_hash);
        match payment_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                if module_bytes.is_empty() {
                    Ok(PaymentInfo {
                        payment: ExecutableItem::StandardPayment,
                        args: args.clone(),
                    })
                } else {
                    Ok(PaymentInfo {
                        payment: ExecutableItem::CustomPayment(module_bytes),
                        args: args.clone(),
                    })
                }
            }
            ExecutableDeployItem::StoredContractByHash { .. }
            | ExecutableDeployItem::StoredContractByName { .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { .. }
            | ExecutableDeployItem::StoredVersionedContractByName { .. }
            | ExecutableDeployItem::Transfer { .. } => Err(InvalidRequest::UnexpectedVariant(
                transaction_hash,
                "payment item".to_string(),
            )),
        }
    }
}

impl<'a> TryFrom<&'a TransactionV1> for PaymentInfo<'a> {
    type Error = InvalidRequest;

    fn try_from(v1_txn: &'a TransactionV1) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::V1(*v1_txn.hash());
        let pricing_mode = v1_txn.pricing_mode();
        let payment_amount = match v1_txn.pricing_mode() {
            PricingMode::Classic {
                payment_amount,
                standard_payment,
                ..
            } => {
                if *standard_payment {
                    return Err(InvalidRequest::UnsupportedMode(
                        transaction_hash,
                        pricing_mode.to_string(),
                    ));
                }
                *payment_amount
            }
            PricingMode::Fixed { .. } | PricingMode::Reserved { .. } => {
                return Err(InvalidRequest::UnsupportedMode(
                    transaction_hash,
                    pricing_mode.to_string(),
                ));
            }
        };

        let payment = match v1_txn.target() {
            TransactionTarget::Session { module_bytes, .. } => {
                ExecutableItem::CustomPayment(module_bytes)
            }
            TransactionTarget::Native | TransactionTarget::Stored { .. } => {
                return Err(InvalidRequest::InvalidTarget(
                    transaction_hash,
                    v1_txn.target().to_string(),
                ));
            }
        };
        let args = runtime_args! { ARG_AMOUNT => U512::from(payment_amount)};
        Ok(PaymentInfo { payment, args })
    }
}
