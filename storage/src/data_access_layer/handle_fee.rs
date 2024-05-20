use crate::{
    data_access_layer::BalanceIdentifier, system::runtime_native::Config as NativeRuntimeConfig,
    tracking_copy::TrackingCopyError,
};
use casper_types::{
    execution::Effects, Digest, EraId, InitiatorAddr, ProtocolVersion, PublicKey, TransactionHash,
    U512,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleFeeMode {
    Pay {
        initiator_addr: Box<InitiatorAddr>,
        source: Box<BalanceIdentifier>,
        target: Box<BalanceIdentifier>,
        amount: U512,
    },
    Burn {
        source: BalanceIdentifier,
        amount: Option<U512>,
    },
    Credit {
        validator: Box<PublicKey>,
        amount: U512,
        era_id: EraId,
    },
}

impl HandleFeeMode {
    pub fn pay(
        initiator_addr: Box<InitiatorAddr>,
        source: BalanceIdentifier,
        target: BalanceIdentifier,
        amount: U512,
    ) -> Self {
        HandleFeeMode::Pay {
            initiator_addr,
            source: Box::new(source),
            target: Box::new(target),
            amount,
        }
    }

    /// What source should be used to burn from, and how much?
    /// If amount is None or greater than the available balance, the full available balance
    /// will be burned. If amount is less than available balance, only that much will be
    /// burned leaving a remaining balance.
    pub fn burn(source: BalanceIdentifier, amount: Option<U512>) -> Self {
        HandleFeeMode::Burn { source, amount }
    }

    /// Applies a staking credit to the imputed proposer for the imputed amount at the end
    /// of the current era when the auction process is executed.
    pub fn credit(validator: Box<PublicKey>, amount: U512, era_id: EraId) -> Self {
        HandleFeeMode::Credit {
            validator,
            amount,
            era_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleFeeRequest {
    /// The runtime config.
    pub(crate) config: NativeRuntimeConfig,
    /// State root hash.
    pub(crate) state_hash: Digest,
    /// The protocol version.
    pub(crate) protocol_version: ProtocolVersion,
    /// Transaction hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Handle fee mode.
    pub(crate) handle_fee_mode: HandleFeeMode,
}

impl HandleFeeRequest {
    /// Creates new request instance with runtime args.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        handle_fee_mode: HandleFeeMode,
    ) -> Self {
        Self {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            handle_fee_mode,
        }
    }

    pub fn config(&self) -> &NativeRuntimeConfig {
        &self.config
    }

    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    pub fn handle_fee_mode(&self) -> &HandleFeeMode {
        &self.handle_fee_mode
    }
}

/// Result enum that represents all possible outcomes of a handle  request.
#[derive(Debug)]
pub enum HandleFeeResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Handle  request succeeded.
    Success { effects: Effects },
    /// Handle  request failed.
    Failure(TrackingCopyError),
}

impl HandleFeeResult {
    /// The effects, if any.
    pub fn effects(&self) -> Effects {
        match self {
            HandleFeeResult::RootNotFound | HandleFeeResult::Failure(_) => Effects::new(),
            HandleFeeResult::Success { effects, .. } => effects.clone(),
        }
    }
}
