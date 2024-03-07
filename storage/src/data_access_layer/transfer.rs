use std::collections::BTreeSet;

use crate::system::runtime_native::{Config as NativeRuntimeConfig, TransferConfig};
use casper_types::{
    account::AccountHash, execution::Effects, BlockTime, Digest, Gas, InitiatorAddr,
    ProtocolVersion, RuntimeArgs, TransactionHash, TransferAddr,
};

use crate::system::transfer::{TransferArgs, TransferError};

/// Transfer details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferRequestArgs {
    Raw(RuntimeArgs),
    Explicit(TransferArgs),
}

/// Request for motes transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRequest {
    /// Config.
    config: NativeRuntimeConfig,
    /// State root hash.
    state_hash: Digest,
    /// Block time represented as a unix timestamp.
    block_time: BlockTime,
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
    /// Cost.
    gas: Gas,
}

impl TransferRequest {
    /// Creates new request object.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        block_time: BlockTime,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        initiator: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        args: TransferArgs,
        gas: Gas,
    ) -> Self {
        let args = TransferRequestArgs::Explicit(args);
        Self {
            config,
            state_hash,
            block_time,
            protocol_version,
            transaction_hash,
            initiator,
            authorization_keys,
            args,
            gas,
        }
    }

    /// Creates new request instance with runtime args.
    #[allow(clippy::too_many_arguments)]
    pub fn with_runtime_args(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        block_time: BlockTime,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        initiator: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        args: RuntimeArgs,
        gas: Gas,
    ) -> Self {
        let args = TransferRequestArgs::Raw(args);
        Self {
            config,
            state_hash,
            block_time,
            protocol_version,
            transaction_hash,
            initiator,
            authorization_keys,
            args,
            gas,
        }
    }

    pub fn config(&self) -> &NativeRuntimeConfig {
        &self.config
    }

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

    /// Returns block time.
    pub fn block_time(&self) -> BlockTime {
        self.block_time
    }

    /// Returns transaction hash.
    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    /// The cost.
    pub fn gas(&self) -> Gas {
        self.gas
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

#[derive(Debug, Clone)]
pub enum TransferResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Transfer succeeded
    Success {
        /// List of transfers that happened during execution.
        transfers: Vec<TransferAddr>,
        /// State hash after transfer is committed to the global state.
        post_state_hash: Digest,
        /// Effects of transfer.
        effects: Effects,
    },
    /// Transfer failed
    Failure(TransferError),
}
