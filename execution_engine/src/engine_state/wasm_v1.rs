use std::{collections::BTreeSet, convert::TryFrom};

use serde::Serialize;
use thiserror::Error;

use casper_storage::data_access_layer::TransferResult;
use casper_types::{
    account::AccountHash, bytesrepr::Bytes, contract_messages::Messages, execution::Effects,
    runtime_args, system::mint::ARG_AMOUNT, BlockTime, DeployHash, Digest, ExecutableDeployItem,
    Gas, InitiatorAddr, Phase, PricingMode, RuntimeArgs, Transaction, TransactionEntryPoint,
    TransactionHash, TransactionInvocationTarget, TransactionSessionKind, TransactionTarget,
    TransactionV1, TransactionV1Hash, TransferAddr, U512,
};

use crate::engine_state::{DeployItem, Error as EngineError, ExecutionResult};

const DEFAULT_ENTRY_POINT: &str = "call";

/// Error returned if constructing a new [`WasmV1Request`] fails.
#[derive(Copy, Clone, Eq, PartialEq, Error, Serialize, Debug)]
pub enum InvalidRequest {
    /// The module bytes for custom payment must not be empty.
    #[error("empty module bytes for custom payment in deploy {0}")]
    EmptyCustomPaymentBytes(DeployHash),
    /// The executable deploy item for custom payment must be the module bytes variant.
    #[error("can only use module bytes for custom payment in deploy {0}")]
    InvalidPaymentDeployItem(DeployHash),
    /// The transaction v1 pricing mode must be the classic variant with `standard_payment` false.
    #[error(
        "can only use classic variant with `standard_payment` false for pricing mode in \
        transaction v1 {0}"
    )]
    InvalidPricingMode(TransactionV1Hash),
    /// The executable deploy item for session cannot be the transfer variant.
    #[error("cannot use transfer variant for session in deploy {0}")]
    InvalidSessionDeployItem(DeployHash),
    /// The transaction v1 target for session cannot be the native variant.
    #[error("cannot use native variant for session target in transaction v1 {0}")]
    InvalidSessionV1Target(TransactionV1Hash),
    /// The transaction v1 entry point for session cannot be one of the native variants.
    #[error("cannot use native variant for session entry point in transaction v1 {0}")]
    InvalidSessionV1EntryPoint(TransactionV1Hash),
}

/// The item to be executed.
#[derive(Debug)]
pub enum ExecutableItem {
    /// A stored entity or package.
    Stored(TransactionInvocationTarget),
    /// Compiled Wasm from a transaction >= V1 as byte code.
    SessionModuleBytes {
        /// The kind of session.
        kind: TransactionSessionKind,
        /// The compiled Wasm.
        module_bytes: Bytes,
    },
    /// Compiled Wasm from a deploy as byte code.
    DeploySessionModuleBytes(Bytes),
    /// Module bytes to be used as custom payment.
    CustomPayment(Bytes),
}

impl ExecutableItem {
    pub(super) fn phase(&self) -> Phase {
        match self {
            ExecutableItem::Stored(_)
            | ExecutableItem::SessionModuleBytes { .. }
            | ExecutableItem::DeploySessionModuleBytes(_) => Phase::Session,
            ExecutableItem::CustomPayment(_) => Phase::Payment,
        }
    }
}

/// A request to execute the given Wasm on the V1 runtime.
#[derive(Debug)]
pub struct WasmV1Request {
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
    pub executable_item: ExecutableItem,
    /// The entry point to call when executing.
    pub entry_point: String,
    /// The runtime args.
    pub args: RuntimeArgs,
    /// The account hashes of the signers of the transaction.
    pub authorization_keys: BTreeSet<AccountHash>,
}

impl WasmV1Request {
    /// Creates a new request from a transaction for use as the session code.
    pub fn new_session(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        txn: Transaction,
    ) -> Result<Self, InvalidRequest> {
        let transaction_hash = txn.hash();
        let initiator_addr = txn.initiator_addr();
        let authorization_keys = txn.signers();

        let session_info = match txn {
            Transaction::Deploy(deploy) => {
                let (hash, _header, _payment, session_deploy_item, _approvals) =
                    deploy.destructure();
                SessionInfo::try_from((session_deploy_item, hash))?
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
        txn: Transaction,
    ) -> Result<Self, InvalidRequest> {
        let transaction_hash = txn.hash();
        let initiator_addr = txn.initiator_addr();
        let authorization_keys = txn.signers();

        let payment_info = match txn {
            Transaction::Deploy(deploy) => {
                let (hash, _header, payment_deploy_item, _session, _approvals) =
                    deploy.destructure();
                PaymentInfo::try_from((payment_deploy_item, hash))?
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
            address,
            session,
            authorization_keys,
            deploy_hash,
            ..
        }: DeployItem,
    ) -> Result<Self, InvalidRequest> {
        let session_info = SessionInfo::try_from((session, deploy_hash))?;
        Ok(Self {
            state_hash,
            block_time,
            transaction_hash: TransactionHash::Deploy(deploy_hash),
            gas_limit,
            initiator_addr: InitiatorAddr::AccountHash(address),
            executable_item: session_info.session,
            entry_point: session_info.entry_point,
            args: session_info.args,
            authorization_keys,
        })
    }

    /// Creates a new request from a deploy item for use as custom payment.
    //
    // TODO - deprecate?
    pub fn new_payment_from_deploy_item(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        DeployItem {
            address,
            payment,
            authorization_keys,
            deploy_hash,
            ..
        }: DeployItem,
    ) -> Result<Self, InvalidRequest> {
        let payment_info = PaymentInfo::try_from((payment, deploy_hash))?;
        Ok(Self {
            state_hash,
            block_time,
            transaction_hash: TransactionHash::Deploy(deploy_hash),
            gas_limit,
            initiator_addr: InitiatorAddr::AccountHash(address),
            executable_item: payment_info.payment,
            entry_point: DEFAULT_ENTRY_POINT.to_string(),
            args: payment_info.args,
            authorization_keys,
        })
    }
}

/// Wasm v1 result.
#[derive(Clone, Debug)]
pub struct WasmV1Result {
    /// List of transfers that happened during execution.
    transfers: Vec<TransferAddr>,
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
    pub fn transfers(&self) -> &Vec<TransferAddr> {
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
struct SessionInfo {
    session: ExecutableItem,
    entry_point: String,
    args: RuntimeArgs,
}

impl TryFrom<(ExecutableDeployItem, DeployHash)> for SessionInfo {
    type Error = InvalidRequest;

    fn try_from(
        (session_item, deploy_hash): (ExecutableDeployItem, DeployHash),
    ) -> Result<Self, Self::Error> {
        let session: ExecutableItem;
        let session_entry_point: String;
        let session_args: RuntimeArgs;
        match session_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                session = ExecutableItem::DeploySessionModuleBytes(module_bytes);
                session_entry_point = DEFAULT_ENTRY_POINT.to_string();
                session_args = args;
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                session =
                    ExecutableItem::Stored(TransactionInvocationTarget::new_invocable_entity(hash));
                session_entry_point = entry_point;
                session_args = args;
            }
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                session = ExecutableItem::Stored(
                    TransactionInvocationTarget::new_invocable_entity_alias(name),
                );
                session_entry_point = entry_point;
                session_args = args;
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => {
                session =
                    ExecutableItem::Stored(TransactionInvocationTarget::new_package(hash, version));
                session_entry_point = entry_point;
                session_args = args;
            }
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => {
                session = ExecutableItem::Stored(TransactionInvocationTarget::new_package_alias(
                    name, version,
                ));
                session_entry_point = entry_point;
                session_args = args;
            }
            ExecutableDeployItem::Transfer { .. } => {
                return Err(InvalidRequest::InvalidSessionDeployItem(deploy_hash));
            }
        }

        Ok(SessionInfo {
            session,
            entry_point: session_entry_point,
            args: session_args,
        })
    }
}

impl TryFrom<TransactionV1> for SessionInfo {
    type Error = InvalidRequest;

    fn try_from(v1_txn: TransactionV1) -> Result<Self, Self::Error> {
        let (hash, _header, body, _approvals) = v1_txn.destructure();
        let (args, target, entry_point, _scheduling) = body.destructure();
        let session = match target {
            TransactionTarget::Native => {
                return Err(InvalidRequest::InvalidSessionV1Target(hash));
            }
            TransactionTarget::Stored { id, .. } => ExecutableItem::Stored(id),
            TransactionTarget::Session {
                kind, module_bytes, ..
            } => ExecutableItem::SessionModuleBytes { kind, module_bytes },
        };

        let TransactionEntryPoint::Custom(entry_point) = entry_point else {
            return Err(InvalidRequest::InvalidSessionV1EntryPoint(hash));
        };

        Ok(SessionInfo {
            session,
            entry_point,
            args,
        })
    }
}

/// Helper struct to carry the appropriate info for converting an `ExecutableDeployItem` or a
/// `TransactionV1` into the corresponding fields of a `WasmV1Request` for execution as custom
/// payment.
struct PaymentInfo {
    payment: ExecutableItem,
    args: RuntimeArgs,
}

impl TryFrom<(ExecutableDeployItem, DeployHash)> for PaymentInfo {
    type Error = InvalidRequest;

    fn try_from(
        (payment_item, deploy_hash): (ExecutableDeployItem, DeployHash),
    ) -> Result<Self, Self::Error> {
        match payment_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                if module_bytes.is_empty() {
                    return Err(InvalidRequest::EmptyCustomPaymentBytes(deploy_hash));
                }
                Ok(PaymentInfo {
                    payment: ExecutableItem::CustomPayment(module_bytes),
                    args,
                })
            }
            ExecutableDeployItem::StoredContractByHash { .. }
            | ExecutableDeployItem::StoredContractByName { .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { .. }
            | ExecutableDeployItem::StoredVersionedContractByName { .. }
            | ExecutableDeployItem::Transfer { .. } => {
                Err(InvalidRequest::InvalidPaymentDeployItem(deploy_hash))
            }
        }
    }
}

impl TryFrom<TransactionV1> for PaymentInfo {
    type Error = InvalidRequest;

    fn try_from(v1_txn: TransactionV1) -> Result<Self, Self::Error> {
        let (hash, header, body, _approvals) = v1_txn.destructure();
        let (_args, target, _entry_point, _scheduling) = body.destructure();
        // TODO - check this is using the correct value (i.e. we don't need to account for gas_price here).
        let payment_amount = match header.pricing_mode() {
            PricingMode::Classic {
                payment_amount,
                standard_payment,
                ..
            } => {
                if *standard_payment {
                    return Err(InvalidRequest::InvalidPricingMode(hash));
                }
                *payment_amount
            }
            PricingMode::Fixed { .. } | PricingMode::Reserved { .. } => {
                return Err(InvalidRequest::InvalidPricingMode(hash));
            }
        };

        let payment = match target {
            // TODO - should we also consider the session kind here?
            TransactionTarget::Session { module_bytes, .. } => {
                ExecutableItem::CustomPayment(module_bytes)
            }
            TransactionTarget::Native | TransactionTarget::Stored { .. } => {
                return Err(InvalidRequest::InvalidSessionV1Target(hash));
            }
        };
        let args = runtime_args! { ARG_AMOUNT => U512::from(payment_amount)};
        Ok(PaymentInfo { payment, args })
    }
}
