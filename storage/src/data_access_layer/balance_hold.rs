use crate::{data_access_layer::BalanceIdentifier, tracking_copy::TrackingCopyError};
use casper_types::{
    account::AccountHash, system::mint::BalanceHoldAddrTag, BlockTime, Digest, EntityAddr,
    ProtocolVersion, PublicKey, TimeDiff, URef, URefAddr, U512,
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
    hold_interval: TimeDiff,
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
        hold_interval: TimeDiff,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            identifier,
            hold_kind,
            hold_amount,
            block_time,
            hold_interval,
            insufficient_handling,
        }
    }

    /// Creates a new [`BalanceHoldRequest`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_purse(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        purse_uref: URef,
        hold_kind: BalanceHoldAddrTag,
        hold_amount: U512,
        block_time: BlockTime,
        hold_interval: TimeDiff,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Purse(purse_uref),
            hold_kind,
            hold_amount,
            block_time,
            hold_interval,
            insufficient_handling,
        }
    }

    /// Creates a new [`BalanceHoldRequest`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_public_key(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        public_key: PublicKey,
        hold_kind: BalanceHoldAddrTag,
        hold_amount: U512,
        block_time: BlockTime,
        hold_interval: TimeDiff,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Public(public_key),
            hold_kind,
            hold_amount,
            block_time,
            hold_interval,
            insufficient_handling,
        }
    }

    /// Creates a new [`BalanceHoldRequest`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_account_hash(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
        hold_kind: BalanceHoldAddrTag,
        hold_amount: U512,
        block_time: BlockTime,
        hold_interval: TimeDiff,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Account(account_hash),
            hold_kind,
            hold_amount,
            block_time,
            hold_interval,
            insufficient_handling,
        }
    }

    /// Creates a new [`BalanceHoldRequest`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_entity_addr(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        entity_addr: EntityAddr,
        hold_kind: BalanceHoldAddrTag,
        hold_amount: U512,
        block_time: BlockTime,
        hold_interval: TimeDiff,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Entity(entity_addr),
            hold_kind,
            hold_amount,
            block_time,
            hold_interval,
            insufficient_handling,
        }
    }

    /// Creates a new [`BalanceHoldRequest`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_internal(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        balance_addr: URefAddr,
        hold_kind: BalanceHoldAddrTag,
        hold_amount: U512,
        block_time: BlockTime,
        hold_interval: TimeDiff,
        insufficient_handling: InsufficientBalanceHandling,
    ) -> Self {
        BalanceHoldRequest {
            state_hash,
            protocol_version,
            identifier: BalanceIdentifier::Internal(balance_addr),
            hold_kind,
            hold_amount,
            block_time,
            hold_interval,
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

    /// Returns the hold interval.
    pub fn hold_interval(&self) -> TimeDiff {
        self.hold_interval
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
pub enum BalanceHoldResult {
    /// Returned if a passed state root hash is not found.
    RootNotFound,
    /// Balance hold successfully placed.
    Success {
        /// Purse total balance.
        total_balance: U512,
        /// Purse available balance after hold placed.
        available_balance: U512,
        /// Were we able to hold the full amount?
        full_amount_held: bool,
    },
    /// Failed to place balance hold.
    Failure(BalanceHoldError),
}
