use crate::{
    system::runtime_native::Config as NativeRuntimeConfig, tracking_copy::TrackingCopyError,
};
use casper_types::{
    account::AccountHash,
    bytesrepr::FromBytes,
    execution::Effects,
    system::{auction, auction::DelegationRate},
    CLTyped, CLValue, Chainspec, Digest, ProtocolVersion, PublicKey, RuntimeArgs,
    TransactionEntryPoint, TransactionHash, U512,
};
use std::collections::BTreeSet;
use tracing::error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuctionMethod {
    ActivateBid {
        validator_public_key: PublicKey,
    },
    AddBid {
        public_key: PublicKey,
        delegation_rate: DelegationRate,
        amount: U512,
        holds_epoch: Option<u64>,
    },
    WithdrawBid {
        public_key: PublicKey,
        amount: U512,
    },
    Delegate {
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
        max_delegators_per_validator: u32,
        minimum_delegation_amount: u64,
        holds_epoch: Option<u64>,
    },
    Undelegate {
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
    },
    Redelegate {
        delegator_public_key: PublicKey,
        validator_public_key: PublicKey,
        amount: U512,
        new_validator: PublicKey,
        minimum_delegation_amount: u64,
    },
}

#[allow(clippy::result_unit_err)]
impl AuctionMethod {
    pub fn from_parts(
        entry_point: TransactionEntryPoint,
        runtime_args: &RuntimeArgs,
        holds_epoch: Option<u64>,
        chainspec: &Chainspec,
    ) -> Result<Self, ()> {
        match entry_point {
            TransactionEntryPoint::Custom(_) | TransactionEntryPoint::Transfer => {
                error!(
                    "attempt to get auction method using a non-auction entry point {}",
                    entry_point
                );
                Err(())
            }
            TransactionEntryPoint::ActivateBid => {
                Self::activate_bid_from_args(runtime_args, chainspec)
            }
            TransactionEntryPoint::AddBid => {
                Self::add_bid_from_args(runtime_args, holds_epoch, chainspec)
            }
            TransactionEntryPoint::WithdrawBid => {
                Self::withdraw_bid_from_args(runtime_args, chainspec)
            }
            TransactionEntryPoint::Delegate => {
                Self::delegate_from_args(runtime_args, holds_epoch, chainspec)
            }
            TransactionEntryPoint::Undelegate => {
                Self::undelegate_from_args(runtime_args, chainspec)
            }
            TransactionEntryPoint::Redelegate => {
                Self::redelegate_from_args(runtime_args, chainspec)
            }
        }
    }

    pub fn activate_bid_from_args(
        runtime_args: &RuntimeArgs,
        _chainspec: &Chainspec,
    ) -> Result<Self, ()> {
        let validator_public_key: PublicKey =
            Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR_PUBLIC_KEY)?;
        Ok(Self::ActivateBid {
            validator_public_key,
        })
    }

    pub fn add_bid_from_args(
        runtime_args: &RuntimeArgs,
        holds_epoch: Option<u64>,
        _chainspec: &Chainspec,
    ) -> Result<Self, ()> {
        let public_key = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
        let delegation_rate = Self::get_named_argument(runtime_args, auction::ARG_DELEGATION_RATE)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
        Ok(Self::AddBid {
            public_key,
            delegation_rate,
            amount,
            holds_epoch,
        })
    }

    pub fn withdraw_bid_from_args(
        runtime_args: &RuntimeArgs,
        _chainspec: &Chainspec,
    ) -> Result<Self, ()> {
        let public_key = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
        Ok(Self::WithdrawBid { public_key, amount })
    }

    pub fn delegate_from_args(
        runtime_args: &RuntimeArgs,
        holds_epoch: Option<u64>,
        chainspec: &Chainspec,
    ) -> Result<Self, ()> {
        let delegator_public_key = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
        let validator_public_key = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

        let max_delegators_per_validator = chainspec.core_config.max_delegators_per_validator;
        let minimum_delegation_amount = chainspec.core_config.minimum_delegation_amount;

        Ok(Self::Delegate {
            delegator_public_key,
            validator_public_key,
            amount,
            max_delegators_per_validator,
            minimum_delegation_amount,
            holds_epoch,
        })
    }

    pub fn undelegate_from_args(
        runtime_args: &RuntimeArgs,
        _chainspec: &Chainspec,
    ) -> Result<Self, ()> {
        let delegator_public_key = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
        let validator_public_key = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

        Ok(Self::Undelegate {
            delegator_public_key,
            validator_public_key,
            amount,
        })
    }

    pub fn redelegate_from_args(
        runtime_args: &RuntimeArgs,
        chainspec: &Chainspec,
    ) -> Result<Self, ()> {
        let delegator_public_key = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
        let validator_public_key = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
        let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
        let new_validator = Self::get_named_argument(runtime_args, auction::ARG_NEW_VALIDATOR)?;

        let minimum_delegation_amount = chainspec.core_config.minimum_delegation_amount;

        Ok(Self::Redelegate {
            delegator_public_key,
            validator_public_key,
            amount,
            new_validator,
            minimum_delegation_amount,
        })
    }

    fn get_named_argument<T: FromBytes + CLTyped>(args: &RuntimeArgs, name: &str) -> Result<T, ()> {
        let arg: CLValue = args.get(name).cloned().ok_or(())?;
        match arg.into_t() {
            Ok(val) => Ok(val),
            Err(err) => {
                error!("{:?}", err);
                Err(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiddingRequest {
    /// The runtime config.
    config: NativeRuntimeConfig,
    /// State root hash.
    state_hash: Digest,
    /// Block time represented as a unix timestamp.
    block_time: u64,
    /// The protocol version.
    protocol_version: ProtocolVersion,
    /// The auction method.
    auction_method: AuctionMethod,
    /// Transaction hash.
    transaction_hash: TransactionHash,
    /// Base account.
    address: AccountHash,
    /// List of authorizing accounts.
    authorization_keys: BTreeSet<AccountHash>,
}

impl BiddingRequest {
    /// Creates new request instance with runtime args.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        block_time: u64,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        address: AccountHash,
        authorization_keys: BTreeSet<AccountHash>,
        auction_method: AuctionMethod,
    ) -> Self {
        Self {
            config,
            state_hash,
            block_time,
            protocol_version,
            transaction_hash,
            address,
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

    pub fn auction_method(&self) -> AuctionMethod {
        self.auction_method.clone()
    }

    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    pub fn address(&self) -> AccountHash {
        self.address
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
    /// Transfer succeeded
    Success {
        // The ret value, if any.
        ret: AuctionMethodRet,
        /// Effects of bidding interaction.
        effects: Effects,
    },
    /// Bidding request failed
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
