use crate::{data_access_layer::BalanceIdentifier, tracking_copy::TrackingCopyError};
use casper_types::{
    execution::Effects, system::mint::BalanceHoldAddrTag, BlockTime, Digest, HoldsEpoch,
    ProtocolVersion, U512,
};
use std::fmt::{Display, Formatter};
use thiserror::Error;

/// How to handle available balance is less than hold amount?
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum InsufficientBalanceHandling {
    /// Hold however much balance remains.
    #[default]
    HoldRemaining,
    /// No operation. Aka, do not place a hold.
    Noop,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BalanceHoldRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    identifier: BalanceIdentifier,
    hold_kind: BalanceHoldAddrTag,
    hold_amount: U512,
    block_time: BlockTime,
    holds_epoch: HoldsEpoch,
    insufficient_handling: InsufficientBalanceHandling,
}

impl BalanceHoldRequest {
    /// Creates a new [`BalanceHoldRequest`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        identifier: BalanceIdentifier,
        hold_kind: BalanceHoldAddrTag,
        hold_amount: U512,
        block_time: BlockTime,
        holds_epoch: HoldsEpoch,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            identifier,
            hold_kind,
            hold_amount,
            block_time,
            holds_epoch,
            insufficient_handling,
        }
    }

    /// Returns a state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns the identifier [`BalanceIdentifier`].
    pub fn identifier(&self) -> &BalanceIdentifier {
        &self.identifier
    }

    /// Returns the hold kind.
    pub fn hold_kind(&self) -> BalanceHoldAddrTag {
        self.hold_kind
    }

    /// Returns the hold amount.
    pub fn hold_amount(&self) -> U512 {
        self.hold_amount
    }

    /// Returns the block time.
    pub fn block_time(&self) -> BlockTime {
        self.block_time
    }

    /// Returns the holds epoch.
    pub fn holds_epoch(&self) -> HoldsEpoch {
        self.holds_epoch
    }

    /// Returns insufficient balance handling option.
    pub fn insufficient_handling(&self) -> InsufficientBalanceHandling {
        self.insufficient_handling
    }
}

/// Possible balance hold errors.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum BalanceHoldError {
    TrackingCopy(TrackingCopyError),
    InsufficientBalance { remaining_balance: U512 },
}

impl Display for BalanceHoldError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BalanceHoldError::TrackingCopy(err) => {
                write!(f, "TrackingCopy: {}", err)
            }
            BalanceHoldError::InsufficientBalance { remaining_balance } => {
                write!(f, "InsufficientBalance: {}", remaining_balance)
            }
        }
    }
}

/// Result enum that represents all possible outcomes of a balance hold request.
#[derive(Debug)]
pub enum BalanceHoldResult {
    /// Returned if a passed state root hash is not found.
    RootNotFound,
    /// Balance hold successfully placed.
    Success {
        /// Purse total balance.
        total_balance: Box<U512>,
        /// Purse available balance after hold placed.
        available_balance: Box<U512>,
        /// How much were we supposed to hold?
        hold: Box<U512>,
        /// How much did we actually hold?
        held: Box<U512>,
        /// Effects of bidding interaction.
        effects: Box<Effects>,
    },
    /// Failed to place balance hold.
    Failure(BalanceHoldError),
}

impl BalanceHoldResult {
    pub fn success(
        total_balance: U512,
        available_balance: U512,
        hold: U512,
        held: U512,
        effects: Effects,
    ) -> Self {
        BalanceHoldResult::Success {
            total_balance: Box::new(total_balance),
            available_balance: Box::new(available_balance),
            hold: Box::new(hold),
            held: Box::new(held),
            effects: Box::new(effects),
        }
    }

    /// Was the hold fully covered?
    pub fn is_fully_covered(&self) -> bool {
        match self {
            BalanceHoldResult::RootNotFound | BalanceHoldResult::Failure(_) => false,
            BalanceHoldResult::Success { hold, held, .. } => hold == held,
        }
    }

    /// Was the hold successful?
    pub fn is_success(&self) -> bool {
        matches!(self, BalanceHoldResult::Success { .. })
    }

    /// Was the root not found?
    pub fn is_root_not_found(&self) -> bool {
        matches!(self, BalanceHoldResult::RootNotFound)
    }

    /// The effects, if any.
    pub fn effects(&self) -> Effects {
        match self {
            BalanceHoldResult::RootNotFound | BalanceHoldResult::Failure(_) => Effects::new(),
            BalanceHoldResult::Success { effects, .. } => *effects.clone(),
        }
    }

    pub fn error_message(&self) -> String {
        match self {
            BalanceHoldResult::Success { hold, held, .. } => {
                if hold == held {
                    String::default()
                } else {
                    format!(
                        "insufficient balance to cover hold amount: {}, held remaining amount: {}",
                        hold, held
                    )
                }
            }
            BalanceHoldResult::RootNotFound => "root not found".to_string(),
            BalanceHoldResult::Failure(bhe) => {
                format!("{:?}", bhe)
            }
        }
    }
}
