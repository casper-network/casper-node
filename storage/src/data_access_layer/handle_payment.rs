use crate::{
    data_access_layer::BalanceIdentifier, system::runtime_native::Config as NativeRuntimeConfig,
    tracking_copy::TrackingCopyError,
};
use casper_types::{execution::Effects, Digest, Gas, ProtocolVersion, TransactionHash, U512};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlePaymentMode {
    Finalize {
        limit: Gas,
        gas_price: Option<u64>,
        cost: U512,
        consumed: Gas,
        source: Box<BalanceIdentifier>,
        target: Box<BalanceIdentifier>,
        holds_epoch: Option<u64>,
    },
    Distribute {
        source: BalanceIdentifier,
        amount: Option<Gas>,
    },
    Burn {
        source: BalanceIdentifier,
        amount: Option<U512>,
    },
}

impl HandlePaymentMode {
    pub fn finalize(
        limit: Gas,
        gas_price: Option<u64>,
        cost: U512,
        consumed: Gas,
        source: BalanceIdentifier,
        target: BalanceIdentifier,
        holds_epoch: Option<u64>,
    ) -> Self {
        HandlePaymentMode::Finalize {
            limit,
            gas_price,
            cost,
            consumed,
            source: Box::new(source),
            target: Box::new(target),
            holds_epoch,
        }
    }

    /// What source should be used to distribute from (typically the Accumulate purse), and how
    /// much? If amount is None or greater than the available balance, the full available
    /// balance will be distributed. If amount is less than available balance, only that much
    /// will be distributed leaving a remaining balance.
    pub fn distribute_accumulated(source: BalanceIdentifier, amount: Option<Gas>) -> Self {
        HandlePaymentMode::Distribute { source, amount }
    }

    /// What source should be used to burn from, and how much?
    /// If amount is None or greater than the available balance, the full available balance
    /// will be burned. If amount is less than available balance, only that much will be
    /// burned leaving a remaining balance.
    pub fn burn(source: BalanceIdentifier, amount: Option<U512>) -> Self {
        HandlePaymentMode::Burn { source, amount }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlePaymentRequest {
    /// The runtime config.
    pub(crate) config: NativeRuntimeConfig,
    /// State root hash.
    pub(crate) state_hash: Digest,
    /// The protocol version.
    pub(crate) protocol_version: ProtocolVersion,
    /// Transaction hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Handle payment mode.
    pub(crate) handle_payment_mode: HandlePaymentMode,
}

impl HandlePaymentRequest {
    /// Creates new request instance with runtime args.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        handle_payment_mode: HandlePaymentMode,
    ) -> Self {
        Self {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            handle_payment_mode,
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

    pub fn handle_payment_mode(&self) -> &HandlePaymentMode {
        &self.handle_payment_mode
    }
}

/// Result enum that represents all possible outcomes of a handle payment request.
#[derive(Debug)]
pub enum HandlePaymentResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Handle payment request succeeded.
    Success { effects: Effects },
    /// Handle payment request failed.
    Failure(TrackingCopyError),
}

impl HandlePaymentResult {
    /// The effects, if any.
    pub fn effects(&self) -> Effects {
        match self {
            HandlePaymentResult::RootNotFound | HandlePaymentResult::Failure(_) => Effects::new(),
            HandlePaymentResult::Success { effects, .. } => effects.clone(),
        }
    }
}
