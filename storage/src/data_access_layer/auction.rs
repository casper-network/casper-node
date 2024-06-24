use std::collections::BTreeSet;

use serde::Serialize;
use thiserror::Error;
use tracing::error;

use casper_types::{
    account::AccountHash,
    bytesrepr::FromBytes,
    execution::Effects,
    system::{auction, auction::DelegationRate},
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuctionMethod {
    ActivateBid {
        validator: PublicKey,
    },
    AddBid {
        public_key: PublicKey,
        delegation_rate: DelegationRate,
        amount: U512,
        minimum_delegation_amount: u64,
        maximum_delegation_amount: u64,
    },
    WithdrawBid {
        public_key: PublicKey,
        amount: U512,
    },
    Delegate {
        delegator: PublicKey,
        validator: PublicKey,
        amount: U512,
        max_delegators_per_validator: u32,
    },
    Undelegate {
        delegator: PublicKey,
        validator: PublicKey,
        amount: U512,
    },
    Redelegate {
        delegator: PublicKey,
        validator: PublicKey,
        amount: U512,
        new_validator: PublicKey,
    },
    ChangeBidPublicKey {
        public_key: PublicKey,
        new_public_key: PublicKey,
    },
}

impl AuctionMethod {
    pub fn from_parts(
        entry_point: TransactionEntryPoint,
        runtime_args: &RuntimeArgs,
        chainspec: &Chainspec,
    ) -> Result<Self, AuctionMethodError> {
        match entry_point {
            TransactionEntryPoint::Call
            | TransactionEntryPoint::Custom(_)
            | TransactionEntryPoint::Transfer
            | TransactionEntryPoint::Instantiate(_)
            | TransactionEntryPoint::DefaultInstantiate => {
                Err(AuctionMethodError::InvalidEntryPoint(entry_point))
            }
            TransactionEntryPoint::ActivateBid => Self::new_activate_bid(runtime_args),
            TransactionEntryPoint::AddBid => Self::new_add_bid(
                runtime_args,
                chainspec.core_config.minimum_delegation_amount,
                chainspec.core_config.maximum_delegation_amount,
            ),
            TransactionEntryPoint::WithdrawBid => Self::new_withdraw_bid(runtime_args),
            TransactionEntryPoint::Delegate => Self::new_delegate(
                runtime_args,
                chainspec.core_config.max_delegators_per_validator,
            ),
            TransactionEntryPoint::Undelegate => Self::new_undelegate(runtime_args),
            TransactionEntryPoint::Redelegate => Self::new_redelegate(runtime_args),
            TransactionEntryPoint::ChangeBidPublicKey => {
                Self::new_change_bid_public_key(runtime_args)
            }
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
        })
    }

    fn new_withdraw_bid(runtime_args: &RuntimeArgs) -> Result<Self, AuctionMethodError> {
        let public_key = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
        Ok(Self::WithdrawBid { public_key, amount })
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

    pub fn config(&self) -> &NativeRuntimeConfig {
        &self.config
    }

    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn auction_method(&self) -> &AuctionMethod {
        &self.auction_method
    }

    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    pub fn initiator(&self) -> &InitiatorAddr {
        &self.initiator
    }

    pub fn authorization_keys(&self) -> &BTreeSet<AccountHash> {
        &self.authorization_keys
    }
}

#[derive(Debug, Clone)]
pub enum AuctionMethodRet {
    Unit,
    UpdatedAmount(U512),
}

#[derive(Debug)]
pub enum BiddingResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Bidding request succeeded
    Success {
        // The ret value, if any.
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
