use std::collections::BTreeSet;

use serde::Serialize;
use thiserror::Error;
use tracing::error;

use casper_types::{
    account::AccountHash,
    bytesrepr::FromBytes,
    execution::Effects,
    system::{
        auction,
        auction::{DelegationRate, Reservation},
    },
    CLTyped, CLValue, CLValueError, Chainspec, Digest, InitiatorAddr, ProtocolVersion, PublicKey,
    RuntimeArgs, TransactionEntryPoint, TransactionHash, U512,
};

use crate::{
    system::runtime_native::Config as NativeRuntimeConfig, tracking_copy::TrackingCopyError,
};

/// An error returned when constructing an [`AuctionMethod`].
#[derive(Clone, Eq, PartialEq, Error, Serialize, Debug)]
pub enum AuctionMethodError {
    /// Provided entry point is not one of the Auction ones.
    #[error("invalid entry point for auction: {0}")]
    InvalidEntryPoint(TransactionEntryPoint),
    /// Required arg missing.
    #[error("missing '{0}' arg")]
    MissingArg(String),
    /// Failed to parse the given arg.
    #[error("failed to parse '{arg}' arg: {error}")]
    CLValue {
        /// The arg name.
        arg: String,
        /// The failure.
        error: CLValueError,
    },
}

/// Auction method to interact with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuctionMethod {
    /// Activate bid.
    ActivateBid {
        /// Validator public key (must match initiating address).
        validator: PublicKey,
    },
    /// Add bid.
    AddBid {
        /// Validator public key (must match initiating address).
        public_key: PublicKey,
        /// Delegation rate for this validator bid.
        delegation_rate: DelegationRate,
        /// Bid amount.
        amount: U512,
        /// Minimum delegation amount for this validator bid.
        minimum_delegation_amount: u64,
        /// Maximum delegation amount for this validator bid.
        maximum_delegation_amount: u64,
        /// The minimum bid amount a validator must submit to have
        /// their bid considered as valid.
        minimum_bid_amount: u64,
    },
    /// Withdraw bid.
    WithdrawBid {
        /// Validator public key.
        public_key: PublicKey,
        /// Bid amount.
        amount: U512,
        /// The minimum bid amount a validator, if a validator reduces their stake
        /// below this amount, then it is treated as a complete withdrawal.
        minimum_bid_amount: u64,
    },
    /// Delegate to validator.
    Delegate {
        /// Delegator public key.
        delegator: PublicKey,
        /// Validator public key.
        validator: PublicKey,
        /// Delegation amount.
        amount: U512,
        /// Max delegators per validator.
        max_delegators_per_validator: u32,
    },
    /// Undelegate from validator.
    Undelegate {
        /// Delegator public key.
        delegator: PublicKey,
        /// Validator public key.
        validator: PublicKey,
        /// Undelegation amount.
        amount: U512,
    },
    /// Undelegate from validator and attempt delegation to new validator after unbonding delay
    /// elapses.
    Redelegate {
        /// Delegator public key.
        delegator: PublicKey,
        /// Validator public key.
        validator: PublicKey,
        /// Redelegation amount.
        amount: U512,
        /// New validator public key.
        new_validator: PublicKey,
    },
    /// Change the public key associated with a validator to a different public key.
    ChangeBidPublicKey {
        /// Current public key.
        public_key: PublicKey,
        /// New public key.
        new_public_key: PublicKey,
    },
    /// Add delegator slot reservations.
    AddReservations {
        /// List of reservations.
        reservations: Vec<Reservation>,
    },
    /// Remove delegator slot reservations for delegators with specified public keys.
    CancelReservations {
        /// Validator public key.
        validator: PublicKey,
        /// List of delegator public keys.
        delegators: Vec<PublicKey>,
        /// Max delegators per validator.
        max_delegators_per_validator: u32,
    },
}

impl AuctionMethod {
    /// Form auction method from parts.
    pub fn from_parts(
        entry_point: TransactionEntryPoint,
        runtime_args: &RuntimeArgs,
        chainspec: &Chainspec,
    ) -> Result<Self, AuctionMethodError> {
        match entry_point {
            TransactionEntryPoint::Call
            | TransactionEntryPoint::Custom(_)
            | TransactionEntryPoint::Transfer => {
                Err(AuctionMethodError::InvalidEntryPoint(entry_point))
            }
            TransactionEntryPoint::ActivateBid => Self::new_activate_bid(runtime_args),
            TransactionEntryPoint::AddBid => Self::new_add_bid(
                runtime_args,
                chainspec.core_config.minimum_delegation_amount,
                chainspec.core_config.maximum_delegation_amount,
                chainspec.core_config.minimum_bid_amount,
            ),
            TransactionEntryPoint::WithdrawBid => {
                Self::new_withdraw_bid(runtime_args, chainspec.core_config.minimum_bid_amount)
            }
            TransactionEntryPoint::Delegate => Self::new_delegate(
                runtime_args,
                chainspec.core_config.max_delegators_per_validator,
            ),
            TransactionEntryPoint::Undelegate => Self::new_undelegate(runtime_args),
            TransactionEntryPoint::Redelegate => Self::new_redelegate(runtime_args),
            TransactionEntryPoint::ChangeBidPublicKey => {
                Self::new_change_bid_public_key(runtime_args)
            }
            TransactionEntryPoint::AddReservations => Self::new_add_reservations(runtime_args),
            TransactionEntryPoint::CancelReservations => Self::new_cancel_reservations(
                runtime_args,
                chainspec.core_config.max_delegators_per_validator,
            ),
        }
    }

    fn new_activate_bid(runtime_args: &RuntimeArgs) -> Result<Self, AuctionMethodError> {
        let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
        Ok(Self::ActivateBid { validator })
    }

    fn new_add_bid(
        runtime_args: &RuntimeArgs,
        global_minimum_delegation: u64,
        global_maximum_delegation: u64,
        global_minimum_bid_amount: u64,
    ) -> Result<Self, AuctionMethodError> {
        let public_key = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
        let delegation_rate = Self::get_named_argument(runtime_args, auction::ARG_DELEGATION_RATE)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
        let minimum_delegation_amount =
            Self::get_named_argument(runtime_args, auction::ARG_MINIMUM_DELEGATION_AMOUNT)
                .unwrap_or(global_minimum_delegation);
        let maximum_delegation_amount =
            Self::get_named_argument(runtime_args, auction::ARG_MAXIMUM_DELEGATION_AMOUNT)
                .unwrap_or(global_maximum_delegation);

        Ok(Self::AddBid {
            public_key,
            delegation_rate,
            amount,
            minimum_delegation_amount,
            maximum_delegation_amount,
            minimum_bid_amount: global_minimum_bid_amount,
        })
    }

    fn new_withdraw_bid(
        runtime_args: &RuntimeArgs,
        global_minimum_bid_amount: u64,
    ) -> Result<Self, AuctionMethodError> {
        let public_key = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
        Ok(Self::WithdrawBid {
            public_key,
            amount,
            minimum_bid_amount: global_minimum_bid_amount,
        })
    }

    fn new_delegate(
        runtime_args: &RuntimeArgs,
        max_delegators_per_validator: u32,
    ) -> Result<Self, AuctionMethodError> {
        let delegator = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
        let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

        Ok(Self::Delegate {
            delegator,
            validator,
            amount,
            max_delegators_per_validator,
        })
    }

    fn new_undelegate(runtime_args: &RuntimeArgs) -> Result<Self, AuctionMethodError> {
        let delegator = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
        let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

        Ok(Self::Undelegate {
            delegator,
            validator,
            amount,
        })
    }

    fn new_redelegate(runtime_args: &RuntimeArgs) -> Result<Self, AuctionMethodError> {
        let delegator = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
        let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
        let new_validator = Self::get_named_argument(runtime_args, auction::ARG_NEW_VALIDATOR)?;

        Ok(Self::Redelegate {
            delegator,
            validator,
            amount,
            new_validator,
        })
    }

    fn new_change_bid_public_key(runtime_args: &RuntimeArgs) -> Result<Self, AuctionMethodError> {
        let public_key = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
        let new_public_key = Self::get_named_argument(runtime_args, auction::ARG_NEW_PUBLIC_KEY)?;

        Ok(Self::ChangeBidPublicKey {
            public_key,
            new_public_key,
        })
    }

    fn new_add_reservations(runtime_args: &RuntimeArgs) -> Result<Self, AuctionMethodError> {
        let reservations = Self::get_named_argument(runtime_args, auction::ARG_RESERVATIONS)?;

        Ok(Self::AddReservations { reservations })
    }

    fn new_cancel_reservations(
        runtime_args: &RuntimeArgs,
        max_delegators_per_validator: u32,
    ) -> Result<Self, AuctionMethodError> {
        let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
        let delegators = Self::get_named_argument(runtime_args, auction::ARG_DELEGATORS)?;

        Ok(Self::CancelReservations {
            validator,
            delegators,
            max_delegators_per_validator,
        })
    }

    fn get_named_argument<T: FromBytes + CLTyped>(
        args: &RuntimeArgs,
        name: &str,
    ) -> Result<T, AuctionMethodError> {
        let arg: &CLValue = args
            .get(name)
            .ok_or_else(|| AuctionMethodError::MissingArg(name.to_string()))?;
        arg.to_t().map_err(|error| AuctionMethodError::CLValue {
            arg: name.to_string(),
            error,
        })
    }
}

/// Bidding request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiddingRequest {
    /// The runtime config.
    pub(crate) config: NativeRuntimeConfig,
    /// State root hash.
    pub(crate) state_hash: Digest,
    /// The protocol version.
    pub(crate) protocol_version: ProtocolVersion,
    /// The auction method.
    pub(crate) auction_method: AuctionMethod,
    /// Transaction hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Base account.
    pub(crate) initiator: InitiatorAddr,
    /// List of authorizing accounts.
    pub(crate) authorization_keys: BTreeSet<AccountHash>,
}

impl BiddingRequest {
    /// Creates new request instance with runtime args.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        initiator: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        auction_method: AuctionMethod,
    ) -> Self {
        Self {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            initiator,
            authorization_keys,
            auction_method,
        }
    }

    /// Returns the config.
    pub fn config(&self) -> &NativeRuntimeConfig {
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

    /// Returns the auction method.
    pub fn auction_method(&self) -> &AuctionMethod {
        &self.auction_method
    }

    /// Returns the transaction hash.
    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    /// Returns the initiator.
    pub fn initiator(&self) -> &InitiatorAddr {
        &self.initiator
    }

    /// Returns the authorization keys.
    pub fn authorization_keys(&self) -> &BTreeSet<AccountHash> {
        &self.authorization_keys
    }
}

/// Auction method ret.
#[derive(Debug, Clone)]
pub enum AuctionMethodRet {
    /// Unit.
    Unit,
    /// Updated amount.
    UpdatedAmount(U512),
}

/// Bidding result.
#[derive(Debug)]
pub enum BiddingResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Bidding request succeeded
    Success {
        /// The ret value, if any.
        ret: AuctionMethodRet,
        /// Effects of bidding interaction.
        effects: Effects,
    },
    /// Bidding request failed.
    Failure(TrackingCopyError),
}

impl BiddingResult {
    /// Is this a success.
    pub fn is_success(&self) -> bool {
        matches!(self, BiddingResult::Success { .. })
    }

    /// Effects.
    pub fn effects(&self) -> Effects {
        match self {
            BiddingResult::RootNotFound | BiddingResult::Failure(_) => Effects::new(),
            BiddingResult::Success { effects, .. } => effects.clone(),
        }
    }
}
