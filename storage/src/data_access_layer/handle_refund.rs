use crate::{
    data_access_layer::BalanceIdentifier, tracking_copy::TrackingCopyError, RuntimeNativeConfig,
};
use casper_types::{
    execution::Effects, Digest, InitiatorAddr, Phase, ProtocolVersion, TransactionHash, Transfer,
    U512,
};
use num_rational::Ratio;

/// Selects refund operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleRefundMode {
    /// This variant will cause the refund amount to be calculated and then burned.
    Burn {
        /// Refund limit.
        limit: U512,
        /// Refund cost.
        cost: U512,
        /// Refund consumed.
        consumed: U512,
        /// Refund gas price.
        gas_price: u8,
        /// Refund source.
        source: Box<BalanceIdentifier>,
        /// Refund ratio.
        ratio: Ratio<u64>,
        /// Available.
        available: U512,
    },
    /// This variant will cause the refund amount to be calculated and the refund to be executed.
    Refund {
        /// Refund initiator.
        initiator_addr: Box<InitiatorAddr>,
        /// Refund limit.
        limit: U512,
        /// Refund cost.
        cost: U512,
        /// Refund consumed.
        consumed: U512,
        /// Refund gas price.
        gas_price: u8,
        /// Refund ratio.
        ratio: Ratio<u64>,
        /// Refund source.
        source: Box<BalanceIdentifier>,
        /// Target for refund.
        target: Box<BalanceIdentifier>,
        /// Available.
        available: U512,
    },
    /// This variant handles the edge case of custom payment plus no fee plus no refund.
    /// This ultimately turns into a hold on the initiator, but it takes extra steps to get there
    /// because the payment has already been fully processed up front and must first be unwound.
    RefundNoFeeCustomPayment {
        /// Refund initiator.
        initiator_addr: Box<InitiatorAddr>,
        /// Refund limit.
        limit: U512,
        /// Refund cost.
        cost: U512,
        /// Refund gas price.
        gas_price: u8,
    },
    /// This variant only calculates and returns the refund amount. It does not
    /// execute a refund.
    CalculateAmount {
        /// Refund limit.
        limit: U512,
        /// Refund cost.
        cost: U512,
        /// Refund consumed.
        consumed: U512,
        /// Refund gas price.
        gas_price: u8,
        /// Refund ratio.
        ratio: Ratio<u64>,
        /// Available.
        available: U512,
    },
    /// This variant will cause the refund purse tracked by handle_payment to be set.
    SetRefundPurse {
        /// Target for refund, which will receive any refunded token while set.
        target: Box<BalanceIdentifier>,
    },
    /// This variant will cause the refund purse tracked by handle_payment to be cleared.
    ClearRefundPurse,
}

impl HandleRefundMode {
    /// Returns the appropriate phase for the mode.
    pub fn phase(&self) -> Phase {
        match self {
            HandleRefundMode::Burn { .. }
            | HandleRefundMode::Refund { .. }
            | HandleRefundMode::RefundNoFeeCustomPayment { .. }
            | HandleRefundMode::CalculateAmount { .. } => Phase::FinalizePayment,

            HandleRefundMode::ClearRefundPurse | HandleRefundMode::SetRefundPurse { .. } => {
                Phase::Payment
            }
        }
    }
}

/// Handle refund request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleRefundRequest {
    /// The runtime config.
    pub(crate) config: RuntimeNativeConfig,
    /// State root hash.
    pub(crate) state_hash: Digest,
    /// The protocol version.
    pub(crate) protocol_version: ProtocolVersion,
    /// Transaction hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Refund handling.
    pub(crate) refund_mode: HandleRefundMode,
}

impl HandleRefundRequest {
    /// Creates a new instance.
    pub fn new(
        config: RuntimeNativeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        refund_mode: HandleRefundMode,
    ) -> Self {
        HandleRefundRequest {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            refund_mode,
        }
    }

    /// Returns a reference to the config.
    pub fn config(&self) -> &RuntimeNativeConfig {
        &self.config
    }

    /// Returns the state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns the protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns the transaction hash.
    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    /// Returns the refund mode.
    pub fn refund_mode(&self) -> &HandleRefundMode {
        &self.refund_mode
    }
}

/// Handle refund result.
#[derive(Debug)]
pub enum HandleRefundResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Handle refund request succeeded.
    Success {
        /// Transfers.
        transfers: Vec<Transfer>,
        /// The effects.
        effects: Effects,
        /// The amount, if any.
        amount: Option<U512>,
    },
    /// Invalid phase selected (programmer error).
    InvalidPhase,
    /// Handle refund request failed.
    Failure(TrackingCopyError),
}

impl HandleRefundResult {
    /// The effects, if any.
    pub fn effects(&self) -> Effects {
        match self {
            HandleRefundResult::RootNotFound
            | HandleRefundResult::InvalidPhase
            | HandleRefundResult::Failure(_) => Effects::new(),
            HandleRefundResult::Success { effects, .. } => effects.clone(),
        }
    }

    /// The refund amount.
    pub fn refund_amount(&self) -> U512 {
        match self {
            HandleRefundResult::RootNotFound
            | HandleRefundResult::InvalidPhase
            | HandleRefundResult::Failure(_) => U512::zero(),
            HandleRefundResult::Success {
                amount: refund_amount,
                ..
            } => refund_amount.unwrap_or(U512::zero()),
        }
    }

    /// The error message, if any.
    pub fn error_message(&self) -> Option<String> {
        match self {
            HandleRefundResult::RootNotFound => Some("root not found".to_string()),
            HandleRefundResult::InvalidPhase => Some("invalid phase selected".to_string()),
            HandleRefundResult::Failure(tce) => Some(format!("{}", tce)),
            HandleRefundResult::Success { .. } => None,
        }
    }
}
