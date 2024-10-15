use crate::{
    data_access_layer::{balance::BalanceFailure, BalanceIdentifier},
    tracking_copy::TrackingCopyError,
};
use casper_types::{
    account::AccountHash,
    execution::Effects,
    system::mint::{BalanceHoldAddr, BalanceHoldAddrTag},
    Digest, ProtocolVersion, StoredValue, U512,
};
use std::fmt::{Display, Formatter};
use thiserror::Error;

/// Balance hold kind.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum BalanceHoldKind {
    /// All balance holds.
    #[default]
    All,
    /// Selection of a specific kind of balance.
    Tag(BalanceHoldAddrTag),
}

impl BalanceHoldKind {
    /// Returns true of imputed tag applies to instance.
    pub fn matches(&self, balance_hold_addr_tag: BalanceHoldAddrTag) -> bool {
        match self {
            BalanceHoldKind::All => true,
            BalanceHoldKind::Tag(tag) => tag == &balance_hold_addr_tag,
        }
    }
}

/// Balance hold mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalanceHoldMode {
    /// Balance hold request.
    Hold {
        /// Balance identifier.
        identifier: BalanceIdentifier,
        /// Hold amount.
        hold_amount: U512,
        /// How should insufficient balance be handled.
        insufficient_handling: InsufficientBalanceHandling,
    },
    /// Clear balance holds.
    Clear {
        /// Identifier of balance to be cleared of holds.
        identifier: BalanceIdentifier,
    },
}

impl Default for BalanceHoldMode {
    fn default() -> Self {
        BalanceHoldMode::Hold {
            insufficient_handling: InsufficientBalanceHandling::HoldRemaining,
            hold_amount: U512::zero(),
            identifier: BalanceIdentifier::Account(AccountHash::default()),
        }
    }
}

/// How to handle available balance is less than hold amount?
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum InsufficientBalanceHandling {
    /// Hold however much balance remains.
    #[default]
    HoldRemaining,
    /// No operation. Aka, do not place a hold.
    Noop,
}

/// Balance hold request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BalanceHoldRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    hold_kind: BalanceHoldKind,
    hold_mode: BalanceHoldMode,
}

impl BalanceHoldRequest {
    /// Creates a new [`BalanceHoldRequest`] for adding a gas balance hold.
    #[allow(clippy::too_many_arguments)]
    pub fn new_gas_hold(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        identifier: BalanceIdentifier,
        hold_amount: U512,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        let hold_kind = BalanceHoldKind::Tag(BalanceHoldAddrTag::Gas);
        let hold_mode = BalanceHoldMode::Hold {
            identifier,
            hold_amount,
            insufficient_handling,
        };
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            hold_kind,
            hold_mode,
        }
    }

    /// Creates a new [`BalanceHoldRequest`] for adding a processing balance hold.
    #[allow(clippy::too_many_arguments)]
    pub fn new_processing_hold(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        identifier: BalanceIdentifier,
        hold_amount: U512,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        let hold_kind = BalanceHoldKind::Tag(BalanceHoldAddrTag::Processing);
        let hold_mode = BalanceHoldMode::Hold {
            identifier,
            hold_amount,
            insufficient_handling,
        };
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            hold_kind,
            hold_mode,
        }
    }

    /// Creates a new [`BalanceHoldRequest`] for clearing holds.
    pub fn new_clear(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        hold_kind: BalanceHoldKind,
        identifier: BalanceIdentifier,
    ) -> Self {
        let hold_mode = BalanceHoldMode::Clear { identifier };
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            hold_kind,
            hold_mode,
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

    /// Balance hold kind.
    pub fn balance_hold_kind(&self) -> BalanceHoldKind {
        self.hold_kind
    }

    /// Balance hold mode.
    pub fn balance_hold_mode(&self) -> BalanceHoldMode {
        self.hold_mode.clone()
    }
}

/// Possible balance hold errors.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum BalanceHoldError {
    /// Tracking copy error.
    TrackingCopy(TrackingCopyError),
    /// Balance error.
    Balance(BalanceFailure),
    /// Insufficient balance error.
    InsufficientBalance {
        /// Remaining balance error.
        remaining_balance: U512,
    },
    /// Unexpected wildcard variant error.
    UnexpectedWildcardVariant, // programmer error,
    /// Unexpected hold value error.
    UnexpectedHoldValue(StoredValue),
}

impl From<BalanceFailure> for BalanceHoldError {
    fn from(be: BalanceFailure) -> Self {
        BalanceHoldError::Balance(be)
    }
}

impl From<TrackingCopyError> for BalanceHoldError {
    fn from(tce: TrackingCopyError) -> Self {
        BalanceHoldError::TrackingCopy(tce)
    }
}

impl Display for BalanceHoldError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BalanceHoldError::TrackingCopy(err) => {
                write!(f, "TrackingCopy: {:?}", err)
            }
            BalanceHoldError::InsufficientBalance { remaining_balance } => {
                write!(f, "InsufficientBalance: {}", remaining_balance)
            }
            BalanceHoldError::UnexpectedWildcardVariant => {
                write!(
                    f,
                    "UnexpectedWildcardVariant: unsupported use of BalanceHoldKind::All"
                )
            }
            BalanceHoldError::Balance(be) => Display::fmt(be, f),
            BalanceHoldError::UnexpectedHoldValue(value) => {
                write!(f, "Found an unexpected hold value in storage: {:?}", value,)
            }
        }
    }
}

/// Result enum that represents all possible outcomes of a balance hold request.
#[derive(Debug)]
pub enum BalanceHoldResult {
    /// Returned if a passed state root hash is not found.
    RootNotFound,
    /// Returned if global state does not have an entry for block time.
    BlockTimeNotFound,
    /// Balance hold successfully placed.
    Success {
        /// Hold addresses, if any.
        holds: Option<Vec<BalanceHoldAddr>>,
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
    /// Success ctor.
    pub fn success(
        holds: Option<Vec<BalanceHoldAddr>>,
        total_balance: U512,
        available_balance: U512,
        hold: U512,
        held: U512,
        effects: Effects,
    ) -> Self {
        BalanceHoldResult::Success {
            holds,
            total_balance: Box::new(total_balance),
            available_balance: Box::new(available_balance),
            hold: Box::new(hold),
            held: Box::new(held),
            effects: Box::new(effects),
        }
    }

    /// Returns the total balance for a [`BalanceHoldResult::Success`] variant.
    pub fn total_balance(&self) -> Option<&U512> {
        match self {
            BalanceHoldResult::Success { total_balance, .. } => Some(total_balance),
            _ => None,
        }
    }

    /// Returns the available balance for a [`BalanceHoldResult::Success`] variant.
    pub fn available_balance(&self) -> Option<&U512> {
        match self {
            BalanceHoldResult::Success {
                available_balance, ..
            } => Some(available_balance),
            _ => None,
        }
    }

    /// Returns the held amount for a [`BalanceHoldResult::Success`] variant.
    pub fn held(&self) -> Option<&U512> {
        match self {
            BalanceHoldResult::Success { held, .. } => Some(held),
            _ => None,
        }
    }

    /// Hold address, if any.
    pub fn holds(&self) -> Option<Vec<BalanceHoldAddr>> {
        match self {
            BalanceHoldResult::RootNotFound
            | BalanceHoldResult::BlockTimeNotFound
            | BalanceHoldResult::Failure(_) => None,
            BalanceHoldResult::Success { holds, .. } => holds.clone(),
        }
    }

    /// Does this result contain any hold addresses?
    pub fn has_holds(&self) -> bool {
        match self.holds() {
            None => false,
            Some(holds) => !holds.is_empty(),
        }
    }

    /// Was the hold fully covered?
    pub fn is_fully_covered(&self) -> bool {
        match self {
            BalanceHoldResult::RootNotFound
            | BalanceHoldResult::BlockTimeNotFound
            | BalanceHoldResult::Failure(_) => false,
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
            BalanceHoldResult::RootNotFound
            | BalanceHoldResult::BlockTimeNotFound
            | BalanceHoldResult::Failure(_) => Effects::new(),
            BalanceHoldResult::Success { effects, .. } => *effects.clone(),
        }
    }

    /// Error message.
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
            BalanceHoldResult::BlockTimeNotFound => "block time not found".to_string(),
            BalanceHoldResult::Failure(bhe) => {
                format!("{:?}", bhe)
            }
        }
    }
}

impl From<BalanceFailure> for BalanceHoldResult {
    fn from(be: BalanceFailure) -> Self {
        BalanceHoldResult::Failure(be.into())
    }
}

impl From<TrackingCopyError> for BalanceHoldResult {
    fn from(tce: TrackingCopyError) -> Self {
        BalanceHoldResult::Failure(tce.into())
    }
}
