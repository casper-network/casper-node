use std::collections::BTreeSet;

use crate::{
    data_access_layer::BalanceIdentifier,
    system::runtime_native::{Config as NativeRuntimeConfig, TransferConfig},
};
use casper_types::{
    account::AccountHash, execution::Effects, Digest, InitiatorAddr, ProtocolVersion, RuntimeArgs,
    TransactionHash, Transfer, U512,
};

use crate::system::transfer::{TransferArgs, TransferError};

/// Transfer arguments using balance identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceIdentifierTransferArgs {
    to: Option<AccountHash>,
    source: BalanceIdentifier,
    target: BalanceIdentifier,
    amount: U512,
    arg_id: Option<u64>,
}

impl BalanceIdentifierTransferArgs {
    /// Ctor.
    pub fn new(
        to: Option<AccountHash>,
        source: BalanceIdentifier,
        target: BalanceIdentifier,
        amount: U512,
        arg_id: Option<u64>,
    ) -> Self {
        BalanceIdentifierTransferArgs {
            to,
            source,
            target,
            amount,
            arg_id,
        }
    }

    /// Get to.
    pub fn to(&self) -> Option<AccountHash> {
        self.to
    }

    /// Get source.
    pub fn source(&self) -> &BalanceIdentifier {
        &self.source
    }

    /// Get target.
    pub fn target(&self) -> &BalanceIdentifier {
        &self.target
    }

    /// Get amount.
    pub fn amount(&self) -> U512 {
        self.amount
    }

    /// Get arg_id.
    pub fn arg_id(&self) -> Option<u64> {
        self.arg_id
    }
}

/// Transfer details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferRequestArgs {
    /// Provides opaque arguments in runtime format.
    Raw(RuntimeArgs),
    /// Provides explicit structured args.
    Explicit(TransferArgs),
    /// Provides support for transfers using balance identifiers.
    /// The source and target purses will get resolved on usage.
    Indirect(Box<BalanceIdentifierTransferArgs>),
}

/// Request for motes transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRequest {
    /// Config.
    config: NativeRuntimeConfig,
    /// State root hash.
    state_hash: Digest,
    /// Protocol version.
    protocol_version: ProtocolVersion,
    /// Transaction hash.
    transaction_hash: TransactionHash,
    /// Base account.
    initiator: InitiatorAddr,
    /// List of authorizing accounts.
    authorization_keys: BTreeSet<AccountHash>,
    /// Args.
    args: TransferRequestArgs,
}

impl TransferRequest {
    /// Creates new request object.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        initiator: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        args: TransferArgs,
    ) -> Self {
        let args = TransferRequestArgs::Explicit(args);
        Self {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            initiator,
            authorization_keys,
            args,
        }
    }

    /// Creates new request instance with runtime args.
    #[allow(clippy::too_many_arguments)]
    pub fn with_runtime_args(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        initiator: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        args: RuntimeArgs,
    ) -> Self {
        let args = TransferRequestArgs::Raw(args);
        Self {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            initiator,
            authorization_keys,
            args,
        }
    }

    /// Creates new request object using balance identifiers.
    #[allow(clippy::too_many_arguments)]
    pub fn new_indirect(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        initiator: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        args: BalanceIdentifierTransferArgs,
    ) -> Self {
        let args = TransferRequestArgs::Indirect(Box::new(args));
        Self {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            initiator,
            authorization_keys,
            args,
        }
    }

    /// Returns a reference to the runtime config.
    pub fn config(&self) -> &NativeRuntimeConfig {
        &self.config
    }

    /// Returns a reference to the transfer config.
    pub fn transfer_config(&self) -> &TransferConfig {
        self.config.transfer_config()
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns initiator.
    pub fn initiator(&self) -> &InitiatorAddr {
        &self.initiator
    }

    /// Returns authorization keys.
    pub fn authorization_keys(&self) -> &BTreeSet<AccountHash> {
        &self.authorization_keys
    }

    /// Returns protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns transaction hash.
    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    /// Returns transfer args.
    pub fn args(&self) -> &TransferRequestArgs {
        &self.args
    }

    /// Into args.
    pub fn into_args(self) -> TransferRequestArgs {
        self.args
    }

    /// Used by `WasmTestBuilder` to set the appropriate state root hash and transfer config before
    /// executing the transfer.
    #[doc(hidden)]
    pub fn set_state_hash_and_config(&mut self, state_hash: Digest, config: NativeRuntimeConfig) {
        self.state_hash = state_hash;
        self.config = config;
    }
}

/// Transfer result.
#[derive(Debug, Clone)]
pub enum TransferResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Transfer succeeded
    Success {
        /// List of transfers that happened during execution.
        transfers: Vec<Transfer>,
        /// Effects of transfer.
        effects: Effects,
    },
    /// Transfer failed
    Failure(TransferError),
}

impl TransferResult {
    /// Returns the effects, if any.
    pub fn effects(&self) -> Effects {
        match self {
            TransferResult::RootNotFound | TransferResult::Failure(_) => Effects::new(),
            TransferResult::Success { effects, .. } => effects.clone(),
        }
    }

    /// Returns transfers.
    pub fn transfers(&self) -> Vec<Transfer> {
        match self {
            TransferResult::RootNotFound | TransferResult::Failure(_) => vec![],
            TransferResult::Success { transfers, .. } => transfers.clone(),
        }
    }

    /// Returns transfer error, if any.
    pub fn error(&self) -> Option<TransferError> {
        if let Self::Failure(error) = self {
            Some(error.clone())
        } else {
            None
        }
    }
}
