use std::{
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
};

use serde::Serialize;
use thiserror::Error;

use casper_storage::data_access_layer::TransferResult;
use casper_types::{
    account::AccountHash, bytesrepr::Bytes, contract_messages::Messages, execution::Effects,
    BlockTime, DeployHash, Digest, ExecutableDeployItem, Gas, InitiatorAddr, Phase, PricingMode,
    RuntimeArgs, Transaction, TransactionCategory, TransactionEntryPoint, TransactionHash,
    TransactionInvocationTarget, TransactionTarget, TransactionV1, Transfer,
};

use crate::engine_state::{DeployItem, Error as EngineError};

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
    /// Unsupported category.
    #[error("invalid category for {0} attempting {1}")]
    InvalidCategory(TransactionHash, String),
}

#[derive(Debug, Clone)]
pub enum SessionKind {
    InstallUpgradeBytecode,
    GenericBytecode,
}

/// The item to be executed.
#[derive(Debug, Clone)]
pub enum ExecutableItem {
    /// Legacy deploy byte code.
    LegacyDeploy(Bytes),
    /// Payment byte code.
    PaymentBytes(Bytes),
    /// Session byte code.
    SessionBytes {
        /// The kind of session.
        kind: SessionKind,
        /// The compiled Wasm.
        module_bytes: Bytes,
    },
    /// An attempt to invoke a stored entity or package.
    Invocation(TransactionInvocationTarget),
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
    /// Execution phase.
    pub phase: Phase,
}

impl WasmV1Request {
    pub(crate) fn new_from_executable_info(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        transaction_hash: TransactionHash,
        initiator_addr: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        executable_info: impl Executable,
    ) -> Self {
        let executable_item = executable_info.item();
        Self {
            state_hash,
            block_time,
            transaction_hash,
            gas_limit,
            initiator_addr,
            authorization_keys,
            executable_item,
            entry_point: executable_info.entry_point().clone(),
            args: executable_info.args().clone(),
            phase: executable_info.phase(),
        }
    }

    /// Creates a new request from a transaction for use as the session code.
    pub fn new_session(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        transaction: &Transaction,
    ) -> Result<Self, InvalidRequest> {
        let info = match transaction {
            Transaction::Deploy(deploy) => {
                SessionInfo::try_from((deploy.session(), deploy.hash()))?
            }
            Transaction::V1(v1_txn) => SessionInfo::try_from(v1_txn)?,
        };

        let transaction_hash = transaction.hash();
        let initiator_addr = transaction.initiator_addr();
        let authorization_keys = transaction.signers();
        Ok(WasmV1Request::new_from_executable_info(
            state_hash,
            block_time,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            info,
        ))
    }

    /// Creates a new request from a transaction for use as custom payment.
    pub fn new_custom_payment(
        state_hash: Digest,
        block_time: BlockTime,
        gas_limit: Gas,
        transaction: &Transaction,
    ) -> Result<Self, InvalidRequest> {
        let info = match transaction {
            Transaction::Deploy(deploy) => {
                PaymentInfo::try_from((deploy.payment(), deploy.hash()))?
            }
            Transaction::V1(v1_txn) => PaymentInfo::try_from(v1_txn)?,
        };

        let transaction_hash = transaction.hash();
        let initiator_addr = transaction.initiator_addr();
        let authorization_keys = transaction.signers();
        Ok(WasmV1Request::new_from_executable_info(
            state_hash,
            block_time,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            info,
        ))
    }

    /// Creates a new request from a deploy item for use as the session code.
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
        }: &DeployItem,
    ) -> Result<Self, InvalidRequest> {
        let info = SessionInfo::try_from((session, deploy_hash))?;
        let transaction_hash = TransactionHash::Deploy(*deploy_hash);
        let initiator_addr = InitiatorAddr::AccountHash(*address);
        let authorization_keys = authorization_keys.clone();
        Ok(WasmV1Request::new_from_executable_info(
            state_hash,
            block_time,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            info,
        ))
    }

    /// Creates a new request from a deploy item for use as custom payment.
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
        }: &DeployItem,
    ) -> Result<Self, InvalidRequest> {
        let info = PaymentInfo::try_from((payment, deploy_hash))?;
        let transaction_hash = TransactionHash::Deploy(*deploy_hash);
        let initiator_addr = InitiatorAddr::AccountHash(*address);
        let authorization_keys = authorization_keys.clone();
        Ok(WasmV1Request::new_from_executable_info(
            state_hash,
            block_time,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            info,
        ))
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
    /// Creates a new instance.
    pub fn new(
        limit: Gas,
        consumed: Gas,
        effects: Effects,
        transfers: Vec<Transfer>,
        messages: Messages,
        error: Option<EngineError>,
    ) -> Self {
        WasmV1Result {
            limit,
            consumed,
            effects,
            transfers,
            messages,
            error,
        }
    }

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

    /// Converts a transfer result to an execution result.
    pub fn from_transfer_result(transfer_result: TransferResult, consumed: Gas) -> Option<Self> {
        // NOTE: for native / wasmless operations limit and consumed are always equal, and
        // we can get away with simplifying to one or the other here.
        // this is NOT true of wasm based operations however.
        match transfer_result {
            TransferResult::RootNotFound => None,
            TransferResult::Success { transfers, effects } => Some(WasmV1Result {
                transfers,
                limit: consumed,
                consumed,
                effects,
                messages: Messages::default(),
                error: None,
            }),
            TransferResult::Failure(te) => {
                Some(WasmV1Result {
                    transfers: vec![],
                    limit: consumed,
                    consumed,
                    effects: Effects::default(), // currently not returning effects on failure
                    messages: Messages::default(),
                    error: Some(EngineError::Transfer(te)),
                })
            }
        }
    }
}

/// Helper struct to carry item, entry_point, and arg info for a `WasmV1Request`.
struct ExecutableInfo {
    item: ExecutableItem,
    entry_point: String,
    args: RuntimeArgs,
}

pub(crate) trait Executable {
    fn item(&self) -> ExecutableItem;
    fn entry_point(&self) -> &String;
    fn args(&self) -> &RuntimeArgs;
    fn phase(&self) -> Phase;
}

/// New type for hanging session specific impl's off of.
struct SessionInfo(ExecutableInfo);

impl Executable for SessionInfo {
    fn item(&self) -> ExecutableItem {
        self.0.item.clone()
    }

    fn entry_point(&self) -> &String {
        &self.0.entry_point
    }

    fn args(&self) -> &RuntimeArgs {
        &self.0.args
    }

    fn phase(&self) -> Phase {
        Phase::Session
    }
}

impl TryFrom<(&ExecutableDeployItem, &DeployHash)> for SessionInfo {
    type Error = InvalidRequest;

    fn try_from(
        (session_item, deploy_hash): (&ExecutableDeployItem, &DeployHash),
    ) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::Deploy(*deploy_hash);
        let session: ExecutableItem;
        let session_entry_point: String;
        let session_args: RuntimeArgs;
        match session_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                session = ExecutableItem::LegacyDeploy(module_bytes.clone());
                session_entry_point = DEFAULT_ENTRY_POINT.to_string();
                session_args = args.clone();
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                session = ExecutableItem::Invocation(
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
                session = ExecutableItem::Invocation(
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
                session = ExecutableItem::Invocation(TransactionInvocationTarget::new_package(
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
                session = ExecutableItem::Invocation(
                    TransactionInvocationTarget::new_package_alias(name.clone(), *version),
                );
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

        Ok(SessionInfo(ExecutableInfo {
            item: session,
            entry_point: session_entry_point,
            args: session_args,
        }))
    }
}

impl TryFrom<&TransactionV1> for SessionInfo {
    type Error = InvalidRequest;

    fn try_from(v1_txn: &TransactionV1) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::V1(*v1_txn.hash());
        let args = v1_txn.args().clone();
        let session = match v1_txn.target() {
            TransactionTarget::Native => {
                return Err(InvalidRequest::InvalidTarget(
                    transaction_hash,
                    v1_txn.target().to_string(),
                ));
            }
            TransactionTarget::Stored { id, .. } => {
                let TransactionEntryPoint::Custom(entry_point) = v1_txn.entry_point() else {
                    return Err(InvalidRequest::InvalidEntryPoint(transaction_hash, v1_txn.entry_point().to_string()));
                };
                let item = ExecutableItem::Invocation(id.clone());
                ExecutableInfo {
                    item,
                    entry_point: entry_point.clone(),
                    args,
                }
            }
            TransactionTarget::Session { module_bytes, .. } => {
                if *v1_txn.entry_point() != TransactionEntryPoint::Call {
                    return Err(InvalidRequest::InvalidEntryPoint(
                        transaction_hash,
                        v1_txn.entry_point().to_string(),
                    ));
                };
                let category = v1_txn.transaction_category();
                let category: TransactionCategory = category.try_into().map_err(|_| {
                    InvalidRequest::InvalidCategory(transaction_hash, category.to_string())
                })?;
                let item = match category {
                    TransactionCategory::InstallUpgrade => ExecutableItem::SessionBytes {
                        kind: SessionKind::InstallUpgradeBytecode,
                        module_bytes: module_bytes.clone(),
                    },
                    TransactionCategory::Large
                    | TransactionCategory::Medium
                    | TransactionCategory::Small => ExecutableItem::SessionBytes {
                        kind: SessionKind::GenericBytecode,
                        module_bytes: module_bytes.clone(),
                    },
                    _ => {
                        return Err(InvalidRequest::InvalidCategory(
                            transaction_hash,
                            category.to_string(),
                        ))
                    }
                };
                ExecutableInfo {
                    item,
                    entry_point: DEFAULT_ENTRY_POINT.to_owned(),
                    args,
                }
            }
        };

        Ok(SessionInfo(session))
    }
}

/// New type for hanging payment specific impl's off of.
struct PaymentInfo(ExecutableInfo);

impl Executable for PaymentInfo {
    fn item(&self) -> ExecutableItem {
        self.0.item.clone()
    }

    fn entry_point(&self) -> &String {
        &self.0.entry_point
    }

    fn args(&self) -> &RuntimeArgs {
        &self.0.args
    }

    fn phase(&self) -> Phase {
        Phase::Payment
    }
}

impl TryFrom<(&ExecutableDeployItem, &DeployHash)> for PaymentInfo {
    type Error = InvalidRequest;

    fn try_from(
        (payment_item, deploy_hash): (&ExecutableDeployItem, &DeployHash),
    ) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::Deploy(*deploy_hash);
        match payment_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                let payment = if module_bytes.is_empty() {
                    return Err(InvalidRequest::UnsupportedMode(
                        transaction_hash,
                        "standard payment is no longer handled by the execution engine".to_string(),
                    ));
                } else {
                    ExecutableItem::PaymentBytes(module_bytes.clone())
                };
                Ok(PaymentInfo(ExecutableInfo {
                    item: payment,
                    entry_point: DEFAULT_ENTRY_POINT.to_string(),
                    args: args.clone(),
                }))
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                args,
                entry_point,
            } => Ok(PaymentInfo(ExecutableInfo {
                item: ExecutableItem::Invocation(TransactionInvocationTarget::ByHash(hash.value())),
                entry_point: entry_point.clone(),
                args: args.clone(),
            })),
            ExecutableDeployItem::StoredContractByName {
                name,
                args,
                entry_point,
            } => Ok(PaymentInfo(ExecutableInfo {
                item: ExecutableItem::Invocation(TransactionInvocationTarget::ByName(name.clone())),
                entry_point: entry_point.clone(),
                args: args.clone(),
            })),
            ExecutableDeployItem::StoredVersionedContractByHash {
                args,
                hash,
                version,
                entry_point,
            } => Ok(PaymentInfo(ExecutableInfo {
                item: ExecutableItem::Invocation(TransactionInvocationTarget::ByPackageHash {
                    addr: hash.value(),
                    version: *version,
                }),
                entry_point: entry_point.clone(),
                args: args.clone(),
            })),
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                args,
                entry_point,
            } => Ok(PaymentInfo(ExecutableInfo {
                item: ExecutableItem::Invocation(TransactionInvocationTarget::ByPackageName {
                    name: name.clone(),
                    version: *version,
                }),
                entry_point: entry_point.clone(),
                args: args.clone(),
            })),
            ExecutableDeployItem::Transfer { .. } => Err(InvalidRequest::UnexpectedVariant(
                transaction_hash,
                "payment item".to_string(),
            )),
        }
    }
}

impl TryFrom<&TransactionV1> for PaymentInfo {
    type Error = InvalidRequest;

    fn try_from(v1_txn: &TransactionV1) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::V1(*v1_txn.hash());
        match v1_txn.pricing_mode() {
            mode @ PricingMode::Classic {
                standard_payment, ..
            } => {
                if *standard_payment {
                    return Err(InvalidRequest::UnsupportedMode(
                        transaction_hash,
                        mode.to_string(),
                    ));
                }
            }
            mode @ PricingMode::Fixed { .. } | mode @ PricingMode::Reserved { .. } => {
                return Err(InvalidRequest::UnsupportedMode(
                    transaction_hash,
                    mode.to_string(),
                ));
            }
        };

        let payment = match v1_txn.target() {
            TransactionTarget::Session { module_bytes, .. } => {
                if *v1_txn.entry_point() != TransactionEntryPoint::Call {
                    return Err(InvalidRequest::InvalidEntryPoint(
                        transaction_hash,
                        v1_txn.entry_point().to_string(),
                    ));
                };
                let item = ExecutableItem::PaymentBytes(module_bytes.clone());
                ExecutableInfo {
                    item,
                    entry_point: DEFAULT_ENTRY_POINT.to_owned(),
                    args: v1_txn.args().clone(),
                }
            }
            TransactionTarget::Native | TransactionTarget::Stored { .. } => {
                return Err(InvalidRequest::InvalidTarget(
                    transaction_hash,
                    v1_txn.target().to_string(),
                ));
            }
        };

        Ok(PaymentInfo(payment))
    }
}
