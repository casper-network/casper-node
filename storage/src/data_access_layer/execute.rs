use std::collections::BTreeSet;

use serde::Serialize;
use thiserror::Error;

use casper_types::{
    account::AccountHash, BlockTime, DeployHash, Digest, ExecutableDeployItem, InitiatorAddr,
    PublicKey, RuntimeArgs, Transaction, TransactionEntryPoint, TransactionHash,
};

/// Error returned if constructing a new [`ExecuteNativeRequest`] fails.
#[derive(Clone, Eq, PartialEq, Error, Serialize, Debug)]
pub enum NewNativeRequestError {
    /// Attempted to construct an [`ExecuteNativeRequest`] from a deploy which is not a transfer.
    #[error("deploy must be a transfer for native execution: {0}")]
    ExpectedTransferDeploy(DeployHash),
    /// Attempted to construct an [`ExecuteNativeRequest`] from a transaction with an invalid entry
    /// point.
    #[error(
        "cannot use custom entry point '{entry_point}' for native execution: {transaction_hash}"
    )]
    InvalidEntryPoint {
        transaction_hash: TransactionHash,
        entry_point: String,
    },
}

/// The entry point of the native contract to call.
#[derive(Debug)]
pub enum NativeEntryPoint {
    /// The `transfer` entry point, used to transfer `Motes` from a source purse to a target purse.
    Transfer,
    /// The `add_bid` entry point, used to create or top off a bid purse.
    AddBid,
    /// The `withdraw_bid` entry point, used to decrease a stake.
    WithdrawBid,
    /// The `delegate` entry point, used to add a new delegator or increase an existing delegator's
    /// stake.
    Delegate,
    /// The `undelegate` entry point, used to reduce a delegator's stake or remove the delegator if
    /// the remaining stake is 0.
    Undelegate,
    /// The `redelegate` entry point, used to reduce a delegator's stake or remove the delegator if
    /// the remaining stake is 0, and after the unbonding delay, automatically delegate to a new
    /// validator.
    Redelegate,
}

/// A request to execute the given native transaction.
#[derive(Debug)]
pub struct ExecuteNativeRequest {
    /// State root hash of the global state in which the transaction will be executed.
    pub parent_state_hash: Digest,
    /// Block time represented as a unix timestamp.
    pub block_time: BlockTime,
    /// The hash identifying the transaction.
    pub transaction_hash: TransactionHash,
    /// The native contract entry point to call.
    pub entry_point: NativeEntryPoint,
    /// The runtime args to be used in execution.
    pub args: RuntimeArgs,
    /// The transaction's initiator.
    pub initiator_addr: InitiatorAddr,
    /// The account hashes of the signers of the transaction.
    pub authorization_keys: BTreeSet<AccountHash>,
    /// The owner of the node that proposed the block containing this request.
    pub proposer: PublicKey,
}

impl ExecuteNativeRequest {
    /// Creates a new execute request.
    pub fn new(
        parent_state_hash: Digest,
        block_time: BlockTime,
        txn: Transaction,
        proposer: PublicKey,
    ) -> Result<Self, NewNativeRequestError> {
        let transaction_hash = txn.hash();
        let initiator_addr = txn.initiator_addr();
        let authorization_keys = txn.signers();
        let entry_point: NativeEntryPoint;
        let args: RuntimeArgs;
        match txn {
            Transaction::Deploy(deploy) => {
                let (deploy_hash, _header, _payment, session, _approvals) = deploy.destructure();
                args = match session {
                    ExecutableDeployItem::Transfer { args } => args,
                    _ => {
                        return Err(NewNativeRequestError::ExpectedTransferDeploy(deploy_hash));
                    }
                };
                entry_point = NativeEntryPoint::Transfer;
            }
            Transaction::V1(v1_txn) => {
                entry_point = match v1_txn.entry_point() {
                    TransactionEntryPoint::Custom(entry_point) => {
                        return Err(NewNativeRequestError::InvalidEntryPoint {
                            transaction_hash,
                            entry_point: entry_point.clone(),
                        })
                    }
                    TransactionEntryPoint::Transfer => NativeEntryPoint::Transfer,
                    TransactionEntryPoint::AddBid => NativeEntryPoint::AddBid,
                    TransactionEntryPoint::WithdrawBid => NativeEntryPoint::WithdrawBid,
                    TransactionEntryPoint::Delegate => NativeEntryPoint::Delegate,
                    TransactionEntryPoint::Undelegate => NativeEntryPoint::Undelegate,
                    TransactionEntryPoint::Redelegate => NativeEntryPoint::Redelegate,
                };
                args = v1_txn.take_args();
            }
        };
        Ok(Self {
            parent_state_hash,
            block_time,
            transaction_hash,
            entry_point,
            args,
            initiator_addr,
            authorization_keys,
            proposer,
        })
    }
}
