use std::{collections::BTreeSet, convert::TryFrom};

use serde::Serialize;
use thiserror::Error;

use casper_types::{
    account::AccountHash, bytesrepr::Bytes, runtime_args, BlockTime, DeployHash, Digest,
    ExecutableDeployItem, InitiatorAddr, PublicKey, RuntimeArgs, Transaction,
    TransactionEntryPoint, TransactionHash, TransactionInvocationTarget, TransactionSessionKind,
    TransactionTarget, TransactionV1Body, TransactionV1Hash, U512,
};

const DEFAULT_ENTRY_POINT: &str = "call";

/// The payment method for the transaction.
#[derive(Debug)]
pub enum Payment {
    /// Standard payment (Wasmless execution).
    Standard,
    /// A stored entity or package to be executed as custom payment.
    Stored(TransactionInvocationTarget),
    /// Compiled Wasm as byte code to be executed as custom payment.
    ModuleBytes(Bytes),
    /// The session from this transaction should be executed as custom payment.
    UseSession,
}

/// The payment-related portion of an `ExecuteRequest`.
pub struct PaymentInfo {
    /// The payment item.
    pub payment: Payment,
    /// The payment entry point.
    pub entry_point: String,
    /// The payment runtime args.
    pub args: RuntimeArgs,
}

impl TryFrom<(ExecutableDeployItem, DeployHash)> for PaymentInfo {
    type Error = NewRequestError;

    fn try_from(
        (payment_item, deploy_hash): (ExecutableDeployItem, DeployHash),
    ) -> Result<Self, Self::Error> {
        let payment: Payment;
        let payment_entry_point: String;
        let payment_args: RuntimeArgs;
        match payment_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                if module_bytes.is_empty() {
                    payment = Payment::Standard;
                } else {
                    payment = Payment::ModuleBytes(module_bytes);
                }
                payment_entry_point = DEFAULT_ENTRY_POINT.to_string();
                payment_args = args;
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                payment = Payment::Stored(TransactionInvocationTarget::new_invocable_entity(hash));
                payment_entry_point = entry_point;
                payment_args = args;
            }
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                payment = Payment::Stored(TransactionInvocationTarget::new_invocable_entity_alias(
                    name,
                ));
                payment_entry_point = entry_point;
                payment_args = args;
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => {
                payment = Payment::Stored(TransactionInvocationTarget::new_package(hash, version));
                payment_entry_point = entry_point;
                payment_args = args;
            }
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => {
                payment = Payment::Stored(TransactionInvocationTarget::new_package_alias(
                    name, version,
                ));
                payment_entry_point = entry_point;
                payment_args = args;
            }
            ExecutableDeployItem::Transfer { .. } => {
                return Err(NewRequestError::InvalidPaymentDeployItem(deploy_hash));
            }
        }
        Ok(PaymentInfo {
            payment,
            entry_point: payment_entry_point,
            args: payment_args,
        })
    }
}

/// The session to be executed.
#[derive(Debug)]
pub enum Session {
    /// A stored entity or package.
    Stored(TransactionInvocationTarget),
    /// Compiled Wasm as byte code.
    ModuleBytes {
        /// The kind of session.
        kind: TransactionSessionKind,
        /// The compiled Wasm.
        module_bytes: Bytes,
    },
}

/// The session-related portion of an `ExecuteRequest`.
pub struct SessionInfo {
    /// The session item.
    pub session: Session,
    /// The session entry point.
    pub entry_point: String,
    /// The session runtime args.
    pub args: RuntimeArgs,
}

impl TryFrom<(ExecutableDeployItem, DeployHash)> for SessionInfo {
    type Error = NewRequestError;

    fn try_from(
        (session_item, deploy_hash): (ExecutableDeployItem, DeployHash),
    ) -> Result<Self, Self::Error> {
        let session: Session;
        let session_entry_point: String;
        let session_args: RuntimeArgs;
        match session_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                session = Session::ModuleBytes {
                    kind: TransactionSessionKind::Standard,
                    module_bytes,
                };
                session_entry_point = DEFAULT_ENTRY_POINT.to_string();
                session_args = args;
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                session = Session::Stored(TransactionInvocationTarget::new_invocable_entity(hash));
                session_entry_point = entry_point;
                session_args = args;
            }
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                session = Session::Stored(TransactionInvocationTarget::new_invocable_entity_alias(
                    name,
                ));
                session_entry_point = entry_point;
                session_args = args;
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => {
                session = Session::Stored(TransactionInvocationTarget::new_package(hash, version));
                session_entry_point = entry_point;
                session_args = args;
            }
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => {
                session = Session::Stored(TransactionInvocationTarget::new_package_alias(
                    name, version,
                ));
                session_entry_point = entry_point;
                session_args = args;
            }
            ExecutableDeployItem::Transfer { .. } => {
                return Err(NewRequestError::InvalidSessionDeployItem(deploy_hash));
            }
        }
        Ok(SessionInfo {
            session,
            entry_point: session_entry_point,
            args: session_args,
        })
    }
}

impl TryFrom<(TransactionV1Body, TransactionV1Hash)> for SessionInfo {
    type Error = NewRequestError;

    fn try_from(
        (txn_v1_body, txn_v1_hash): (TransactionV1Body, TransactionV1Hash),
    ) -> Result<Self, Self::Error> {
        let (args, target, entry_point, _scheduling) = txn_v1_body.destructure();

        let session: Session;
        match target {
            TransactionTarget::Native => {
                return Err(NewRequestError::InvalidSessionV1Target(txn_v1_hash))
            }
            TransactionTarget::Stored { id, .. } => {
                session = Session::Stored(id);
            }
            TransactionTarget::Session {
                kind, module_bytes, ..
            } => session = Session::ModuleBytes { kind, module_bytes },
        }

        let TransactionEntryPoint::Custom(session_entry_point) = entry_point else {
            return Err(NewRequestError::InvalidSessionV1EntryPoint(txn_v1_hash));
        };

        Ok(SessionInfo {
            session,
            entry_point: session_entry_point,
            args,
        })
    }
}

/// Error returned if constructing a new [`ExecuteRequest`] fails.
#[derive(Copy, Clone, Eq, PartialEq, Error, Serialize, Debug)]
pub enum NewRequestError {
    /// The executable deploy item for payment cannot be the transfer variant.
    #[error("cannot use transfer variant for payment in deploy {0}")]
    InvalidPaymentDeployItem(DeployHash),
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

/// A request to execute the given transaction.
#[derive(Debug)]
pub struct ExecuteRequest {
    /// State root hash of the global state in which the transaction will be executed.
    pub pre_state_hash: Digest,
    /// Block time represented as a unix timestamp.
    pub block_time: BlockTime,
    /// The hash identifying the transaction.
    pub transaction_hash: TransactionHash,
    /// The number of Motes per unit of Gas to be paid for execution.
    pub gas_price: u64,
    /// The transaction's initiator.
    pub initiator_addr: InitiatorAddr,
    /// The payment kind.
    pub payment: Payment,
    /// The entry point to call when executing the payment.
    pub payment_entry_point: String,
    /// The payment runtime args.
    pub payment_args: RuntimeArgs,
    /// The session kind.
    pub session: Session,
    /// The entry point to call when executing the session.
    pub session_entry_point: String,
    /// The session runtime args.
    pub session_args: RuntimeArgs,
    /// The account hashes of the signers of the transaction.
    pub authorization_keys: BTreeSet<AccountHash>,
    /// The owner of the node that proposed the block containing this request.
    pub proposer: PublicKey,
}

impl ExecuteRequest {
    /// Creates a new execute request.
    pub fn new(
        parent_state_hash: Digest,
        block_time: BlockTime,
        txn: Transaction,
        proposer: PublicKey,
    ) -> Result<Self, NewRequestError> {
        let transaction_hash = txn.hash();
        let gas_price = txn.gas_price();
        let initiator_addr = txn.initiator_addr();
        let authorization_keys = txn.signers();

        let payment_info: PaymentInfo;
        let session_info: SessionInfo;
        match txn {
            Transaction::Deploy(deploy) => {
                let (hash, _header, payment, session, _approvals) = deploy.destructure();
                payment_info = PaymentInfo::try_from((payment, hash))?;
                session_info = SessionInfo::try_from((session, hash))?;
            }
            Transaction::V1(v1_txn) => {
                let (hash, header, body, _approvals) = v1_txn.destructure();
                match header.payment_amount() {
                    Some(amount) => {
                        payment_info = PaymentInfo {
                            payment: Payment::Standard,
                            entry_point: DEFAULT_ENTRY_POINT.to_string(),
                            args: runtime_args! { "amount" => U512::from(amount) },
                        };
                    }
                    None => {
                        // If the payment amount is `None`, the provided session will be invoked
                        // during the payment and session phases.
                        payment_info = PaymentInfo {
                            payment: Payment::UseSession,
                            entry_point: DEFAULT_ENTRY_POINT.to_string(),
                            args: RuntimeArgs::new(),
                        };
                    }
                };
                session_info = SessionInfo::try_from((body, hash))?;
            }
        };

        Ok(Self {
            pre_state_hash: parent_state_hash,
            block_time,
            transaction_hash,
            gas_price,
            initiator_addr,
            payment: payment_info.payment,
            payment_entry_point: payment_info.entry_point,
            payment_args: payment_info.args,
            session: session_info.session,
            session_entry_point: session_info.entry_point,
            session_args: session_info.args,
            authorization_keys,
            proposer,
        })
    }
}
