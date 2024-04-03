use crate::{
    data_access_layer::BalanceIdentifier, system::runtime_native::Config as NativeRuntimeConfig,
    tracking_copy::TrackingCopyError,
};
use casper_types::{
    execution::Effects, Digest, InitiatorAddr, ProtocolVersion, TransactionHash, U512,
};
use num_rational::Ratio;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleRefundMode {
    Burn {
        limit: U512,
        consumed: U512,
        gas_price: u8,
        source: Box<BalanceIdentifier>,
        ratio: Ratio<u64>,
    },
    Refund {
        initiator_addr: Box<InitiatorAddr>,
        limit: U512,
        consumed: U512,
        gas_price: u8,
        source: Box<BalanceIdentifier>,
        target: Box<BalanceIdentifier>,
        ratio: Ratio<u64>,
    },
    SetRefundPurse {
        target: Box<BalanceIdentifier>,
    },
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

pub enum HandleRefundResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Handle refund request succeeded.
    Success { effects: Effects },
    /// Handle refund request failed.
    Failure(TrackingCopyError),
}

impl HandleRefundResult {
    /// The effects, if any.
    pub fn effects(&self) -> Effects {
        match self {
            HandleRefundResult::RootNotFound | HandleRefundResult::Failure(_) => Effects::new(),
            HandleRefundResult::Success { effects, .. } => effects.clone(),
        }
    }
}
