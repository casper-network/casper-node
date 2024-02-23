use std::collections::BTreeSet;

use casper_types::{
    account::AccountHash, execution::Effects, BlockTime, Digest, Gas, InitiatorAddr,
    ProtocolVersion, PublicKey, RuntimeArgs, TransactionHash, TransferAddr,
};

use crate::system::transfer::{TransferArgs, TransferError};

/// Transfer details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferRequestArgs {
    Raw(RuntimeArgs),
    Explicit(TransferArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferConfig {
    Administered {
        administrative_accounts: BTreeSet<AccountHash>,
        allow_unrestricted_transfer: bool,
    },
    Unadministered,
}

impl TransferConfig {
    /// Returns a new instance.
    pub fn new(
        administrative_accounts: BTreeSet<AccountHash>,
        allow_unrestricted_transfer: bool,
    ) -> Self {
        if administrative_accounts.is_empty() && allow_unrestricted_transfer {
            TransferConfig::Unadministered
        } else {
            TransferConfig::Administered {
                administrative_accounts,
                allow_unrestricted_transfer,
            }
        }
    }

    /// Does account hash belong to an administrative account?
    pub fn is_administrator(&self, account_hash: &AccountHash) -> bool {
        match self {
            TransferConfig::Administered {
                administrative_accounts,
                ..
            } => administrative_accounts.contains(account_hash),
            TransferConfig::Unadministered => false,
        }
    }

    /// Administrative accounts, if any.
    pub fn administrative_accounts(&self) -> BTreeSet<AccountHash> {
        match self {
            TransferConfig::Administered {
                administrative_accounts,
                ..
            } => administrative_accounts.clone(),
            TransferConfig::Unadministered => BTreeSet::default(),
        }
    }

    /// Allow unrestricted transfers.
    pub fn allow_unrestricted_transfers(&self) -> bool {
        match self {
            TransferConfig::Administered {
                allow_unrestricted_transfer,
                ..
            } => *allow_unrestricted_transfer,
            TransferConfig::Unadministered => true,
        }
    }

    /// Restricted transfer should be enforced.
    pub fn enforce_transfer_restrictions(&self, account_hash: &AccountHash) -> bool {
        !self.allow_unrestricted_transfers() && !self.is_administrator(account_hash)
    }
}

/// Request for motes transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRequest {
    /// Config.
    config: TransferConfig,
    /// State root hash.
    state_hash: Digest,
    /// Block time represented as a unix timestamp.
    block_time: BlockTime,
    /// Protocol version.
    protocol_version: ProtocolVersion,
    /// Public key of the proposer.
    proposer: PublicKey,
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
        config: TransferConfig,
        state_hash: Digest,
        block_time: BlockTime,
        protocol_version: ProtocolVersion,
        proposer: PublicKey,
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
            proposer,
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
        config: TransferConfig,
        state_hash: Digest,
        block_time: BlockTime,
        protocol_version: ProtocolVersion,
        proposer: PublicKey,
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
            proposer,
            transaction_hash,
            initiator,
            authorization_keys,
            args,
            gas,
        }
    }

    pub fn config(&self) -> &TransferConfig {
        &self.config
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

    /// Returns proposer.
    pub fn proposer(&self) -> &PublicKey {
        &self.proposer
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
    pub fn set_state_hash_and_config(&mut self, state_hash: Digest, config: TransferConfig) {
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
