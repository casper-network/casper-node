use crate::{
    data_access_layer::BalanceIdentifier, system::runtime_native::Config as NativeRuntimeConfig,
    tracking_copy::TrackingCopyError,
};
use casper_types::{
    execution::Effects, Digest, InitiatorAddr, Phase, ProtocolVersion, TransactionHash, U512,
};
use num_rational::Ratio;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleRefundMode {
    Burn {
        limit: U512,
        cost: U512,
        consumed: U512,
        gas_price: u8,
        source: Box<BalanceIdentifier>,
        ratio: Ratio<u64>,
    },
    Refund {
        initiator_addr: Box<InitiatorAddr>,
        limit: U512,
        cost: U512,
        consumed: U512,
        gas_price: u8,
        ratio: Ratio<u64>,
        source: Box<BalanceIdentifier>,
        target: Box<BalanceIdentifier>,
    },
    CustomHold {
        initiator_addr: Box<InitiatorAddr>,
        limit: U512,
        cost: U512,
        gas_price: u8,
    },
    RefundAmount {
        limit: U512,
        cost: U512,
        consumed: U512,
        gas_price: u8,
        ratio: Ratio<u64>,
        source: Box<BalanceIdentifier>,
    },
    SetRefundPurse {
        target: Box<BalanceIdentifier>,
    },
    ClearRefundPurse,
}

impl HandleRefundMode {
    /// Returns the appropriate phase for the mode.
    pub fn phase(&self) -> Phase {
        match self {
            HandleRefundMode::ClearRefundPurse
            | HandleRefundMode::Burn { .. }
            | HandleRefundMode::Refund { .. }
            | HandleRefundMode::CustomHold { .. }
            | HandleRefundMode::RefundAmount { .. } => Phase::FinalizePayment,
            HandleRefundMode::SetRefundPurse { .. } => Phase::Payment,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleRefundRequest {
    /// The runtime config.
    pub(crate) config: NativeRuntimeConfig,
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
        config: NativeRuntimeConfig,
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

    pub fn refund_mode(&self) -> &HandleRefundMode {
        &self.refund_mode
    }
}

#[derive(Debug)]
pub enum HandleRefundResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Handle refund request succeeded.
    Success {
        effects: Effects,
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
