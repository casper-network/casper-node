use crate::{
    account::{AccountHash, ACCOUNT_HASH_LENGTH},
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    system::auction::{error::Error, DelegatorKind},
    EraId, Key, KeyTag, PublicKey, URefAddr,
};
use alloc::vec::Vec;
use core::fmt::{Debug, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "testing", test))]
use strum::EnumIter;

const UNIFIED_TAG: u8 = 0;
const VALIDATOR_TAG: u8 = 1;
const DELEGATED_ACCOUNT_TAG: u8 = 2;
const DELEGATED_PURSE_TAG: u8 = 3;
const CREDIT_TAG: u8 = 4;
const RESERVATION_ACCOUNT_TAG: u8 = 5;
const RESERVATION_PURSE_TAG: u8 = 6;
const UNBOND_ACCOUNT_TAG: u8 = 7;
const UNBOND_PURSE_TAG: u8 = 8;
const VALIDATOR_REV_PURSE_TAG: u8 = 9;

/// Serialization tag for BidAddr variants.
#[derive(
    Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize,
)]
#[repr(u8)]
#[cfg_attr(any(feature = "testing", test), derive(EnumIter))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BidAddrTag {
    /// BidAddr for legacy unified bid.
    Unified = UNIFIED_TAG,
    /// BidAddr for validator bid.
    #[default]
    Validator = VALIDATOR_TAG,
    /// BidAddr for delegated account bid.
    DelegatedAccount = DELEGATED_ACCOUNT_TAG,
    /// BidAddr for delegated purse bid.
    DelegatedPurse = DELEGATED_PURSE_TAG,

    /// BidAddr for auction credit.
    Credit = CREDIT_TAG,

    /// BidAddr for reserved delegation account bid.
    ReservedDelegationAccount = RESERVATION_ACCOUNT_TAG,
    /// BidAddr for reserved delegation purse bid.
    ReservedDelegationPurse = RESERVATION_PURSE_TAG,
    /// BidAddr for unbonding accounts.
    UnbondAccount = UNBOND_ACCOUNT_TAG,
    /// BidAddr for unbonding purses.
    UnbondPurse = UNBOND_PURSE_TAG,
    /// BidAddr for reverse validator look up.
    ValidatorRev = VALIDATOR_REV_PURSE_TAG,
}

impl Display for BidAddrTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = match self {
            BidAddrTag::Unified => UNIFIED_TAG,
            BidAddrTag::Validator => VALIDATOR_TAG,
            BidAddrTag::DelegatedAccount => DELEGATED_ACCOUNT_TAG,
            BidAddrTag::DelegatedPurse => DELEGATED_PURSE_TAG,

            BidAddrTag::Credit => CREDIT_TAG,
            BidAddrTag::ReservedDelegationAccount => RESERVATION_ACCOUNT_TAG,
            BidAddrTag::ReservedDelegationPurse => RESERVATION_PURSE_TAG,
            BidAddrTag::UnbondAccount => UNBOND_ACCOUNT_TAG,
            BidAddrTag::UnbondPurse => UNBOND_PURSE_TAG,
            BidAddrTag::ValidatorRev => VALIDATOR_REV_PURSE_TAG,
        };
        write!(f, "{}", base16::encode_lower(&[tag]))
    }
}

impl BidAddrTag {
    /// The length in bytes of a [`BidAddrTag`].
    pub const BID_ADDR_TAG_LENGTH: usize = 1;

    /// Attempts to map `BidAddrTag` from a u8.
    pub fn try_from_u8(value: u8) -> Option<Self> {
        // TryFrom requires std, so doing this instead.
        if value == UNIFIED_TAG {
            return Some(BidAddrTag::Unified);
        }
        if value == VALIDATOR_TAG {
            return Some(BidAddrTag::Validator);
        }
        if value == DELEGATED_ACCOUNT_TAG {
            return Some(BidAddrTag::DelegatedAccount);
        }
        if value == DELEGATED_PURSE_TAG {
            return Some(BidAddrTag::DelegatedPurse);
        }

        if value == CREDIT_TAG {
            return Some(BidAddrTag::Credit);
        }
        if value == RESERVATION_ACCOUNT_TAG {
            return Some(BidAddrTag::ReservedDelegationAccount);
        }
        if value == RESERVATION_PURSE_TAG {
            return Some(BidAddrTag::ReservedDelegationPurse);
        }
        if value == UNBOND_ACCOUNT_TAG {
            return Some(BidAddrTag::UnbondAccount);
        }
        if value == UNBOND_PURSE_TAG {
            return Some(BidAddrTag::UnbondPurse);
        }
        if value == VALIDATOR_REV_PURSE_TAG {
            return Some(BidAddrTag::ValidatorRev);
        }
        None
    }
}

/// Bid Address
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BidAddr {
    /// Unified BidAddr.
    Unified(AccountHash),
    /// Validator BidAddr.
    Validator(AccountHash),
    /// Delegated account BidAddr.
    DelegatedAccount {
        /// The validator addr.
        validator: AccountHash,
        /// The delegator addr.
        delegator: AccountHash,
    },
    /// Delegated purse BidAddr.
    DelegatedPurse {
        /// The validator addr.
        validator: AccountHash,
        /// The delegated purse addr.
        delegator: URefAddr,
    },
    /// Validator credit BidAddr.
    Credit {
        /// The validator addr.
        validator: AccountHash,
        /// The era id.
        era_id: EraId,
    },
    /// Reserved delegation account BidAddr
    ReservedDelegationAccount {
        /// The validator addr.
        validator: AccountHash,
        /// The delegator addr.
        delegator: AccountHash,
    },
    /// Reserved delegation purse BidAddr
    ReservedDelegationPurse {
        /// The validator addr.
        validator: AccountHash,
        /// The delegated purse addr.
        delegator: URefAddr,
    },
    UnbondAccount {
        /// The validator.
        validator: AccountHash,
        /// The unbonder.
        unbonder: AccountHash,
    },
    UnbondPurse {
        /// The validator.
        validator: AccountHash,
        /// The unbonder.
        unbonder: URefAddr,
    },
    /// Validator BidAddr for reverse look up.
    /// For instance, in the case of a changed public key.
    ValidatorRev(AccountHash),
}

impl BidAddr {
    /// The length in bytes of a [`BidAddr`] for a validator bid.
    pub const VALIDATOR_BID_ADDR_LENGTH: usize =
        ACCOUNT_HASH_LENGTH + BidAddrTag::BID_ADDR_TAG_LENGTH;

    /// The length in bytes of a [`BidAddr`] for a delegator bid.
    pub const DELEGATOR_BID_ADDR_LENGTH: usize =
        (ACCOUNT_HASH_LENGTH * 2) + BidAddrTag::BID_ADDR_TAG_LENGTH;

    /// Constructs a new [`BidAddr`] instance from a validator's [`AccountHash`].
    pub const fn new_validator_addr(validator: [u8; ACCOUNT_HASH_LENGTH]) -> Self {
        BidAddr::Validator(AccountHash::new(validator))
    }

    /// Constructs a new [`BidAddr`] instance from a validator's [`AccountHash`].
    pub const fn new_validator_rev_addr(validator: [u8; ACCOUNT_HASH_LENGTH]) -> Self {
        BidAddr::ValidatorRev(AccountHash::new(validator))
    }

    /// Constructs a new [`BidAddr`] instance from a validator's [`PublicKey`].
    pub fn new_validator_addr_from_public_key(validator_public_key: PublicKey) -> Self {
        BidAddr::Validator(validator_public_key.to_account_hash())
    }

    /// Constructs a new [`BidAddr`] instance from a validator's [`PublicKey`].
    pub fn new_validator_rev_addr_from_public_key(validator_public_key: PublicKey) -> Self {
        BidAddr::ValidatorRev(validator_public_key.to_account_hash())
    }

    /// Constructs a new [`BidAddr::DelegatedAccount`] instance from the [`AccountHash`] pair of a
    /// validator and a delegator.
    pub const fn new_delegator_account_addr(
        pair: ([u8; ACCOUNT_HASH_LENGTH], [u8; ACCOUNT_HASH_LENGTH]),
    ) -> Self {
        BidAddr::DelegatedAccount {
            validator: AccountHash::new(pair.0),
            delegator: AccountHash::new(pair.1),
        }
    }

    /// Constructs a new [`BidAddr::ReservedDelegationAccount`] instance from the [`AccountHash`]
    /// pair of a validator and a delegator.
    pub const fn new_reservation_account_addr(
        pair: ([u8; ACCOUNT_HASH_LENGTH], [u8; ACCOUNT_HASH_LENGTH]),
    ) -> Self {
        BidAddr::ReservedDelegationAccount {
            validator: AccountHash::new(pair.0),
            delegator: AccountHash::new(pair.1),
        }
    }

    #[allow(missing_docs)]
    pub const fn legacy(validator: [u8; ACCOUNT_HASH_LENGTH]) -> Self {
        BidAddr::Unified(AccountHash::new(validator))
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_delegator_kind_relaxed(
        validator: AccountHash,
        delegator_kind: &DelegatorKind,
    ) -> Self {
        match delegator_kind {
            DelegatorKind::PublicKey(pk) => BidAddr::DelegatedAccount {
                validator,
                delegator: pk.to_account_hash(),
            },
            DelegatorKind::Purse(addr) => BidAddr::DelegatedPurse {
                validator,
                delegator: *addr,
            },
        }
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_delegator_kind(validator: &PublicKey, delegator_kind: &DelegatorKind) -> Self {
        Self::new_delegator_kind_relaxed(validator.to_account_hash(), delegator_kind)
    }

    /// Create a new instance of a [`BidAddr`] for delegator unbonds.
    pub fn new_delegator_unbond_relaxed(
        validator: AccountHash,
        delegator_kind: &DelegatorKind,
    ) -> Self {
        match &delegator_kind {
            DelegatorKind::PublicKey(pk) => BidAddr::UnbondAccount {
                validator,
                unbonder: pk.to_account_hash(),
            },
            DelegatorKind::Purse(addr) => BidAddr::UnbondPurse {
                validator,
                unbonder: *addr,
            },
        }
    }

    /// Create a new instance of a [`BidAddr`] for delegator unbonds.
    pub fn new_delegator_unbond(validator: &PublicKey, delegator_kind: &DelegatorKind) -> Self {
        Self::new_delegator_unbond_relaxed(validator.to_account_hash(), delegator_kind)
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_from_public_keys(
        validator: &PublicKey,
        maybe_delegator: Option<&PublicKey>,
    ) -> Self {
        if let Some(delegator) = maybe_delegator {
            BidAddr::DelegatedAccount {
                validator: AccountHash::from(validator),
                delegator: AccountHash::from(delegator),
            }
        } else {
            BidAddr::Validator(AccountHash::from(validator))
        }
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_purse_delegation(validator: &PublicKey, delegator: URefAddr) -> Self {
        BidAddr::DelegatedPurse {
            validator: validator.to_account_hash(),
            delegator,
        }
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_credit(validator: &PublicKey, era_id: EraId) -> Self {
        BidAddr::Credit {
            validator: AccountHash::from(validator),
            era_id,
        }
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_reservation_account(validator: &PublicKey, delegator: &PublicKey) -> Self {
        BidAddr::ReservedDelegationAccount {
            validator: validator.into(),
            delegator: delegator.into(),
        }
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_reservation_purse(validator: &PublicKey, delegator: URefAddr) -> Self {
        BidAddr::ReservedDelegationPurse {
            validator: validator.to_account_hash(),
            delegator,
        }
    }

    /// Create a new instance of a [`BidAddr`].
    pub fn new_unbond_account(validator: PublicKey, unbonder: PublicKey) -> Self {
        BidAddr::UnbondAccount {
            validator: validator.to_account_hash(),
            unbonder: unbonder.to_account_hash(),
        }
    }

    /// Returns the common prefix of all delegated accounts to the cited validator.
    pub fn delegated_account_prefix(&self) -> Result<Vec<u8>, Error> {
        let validator = self.validator_account_hash();
        let mut ret = Vec::with_capacity(validator.serialized_length() + 2);
        ret.push(KeyTag::BidAddr as u8);
        ret.push(BidAddrTag::DelegatedAccount as u8);
        validator.write_bytes(&mut ret)?;
        Ok(ret)
    }

    /// Returns the common prefix of all delegated purses to the cited validator.
    pub fn delegated_purse_prefix(&self) -> Result<Vec<u8>, Error> {
        let validator = self.validator_account_hash();
        let mut ret = Vec::with_capacity(validator.serialized_length() + 2);
        ret.push(KeyTag::BidAddr as u8);
        ret.push(BidAddrTag::DelegatedPurse as u8);
        validator.write_bytes(&mut ret)?;
        Ok(ret)
    }

    /// Returns the common prefix of all reservations for accounts to the cited validator.
    pub fn reserved_account_prefix(&self) -> Result<Vec<u8>, Error> {
        let validator = self.validator_account_hash();
        let mut ret = Vec::with_capacity(validator.serialized_length() + 2);
        ret.push(KeyTag::BidAddr as u8);
        ret.push(BidAddrTag::ReservedDelegationAccount as u8);
        validator.write_bytes(&mut ret)?;
        Ok(ret)
    }

    /// Returns the common prefix of all reservations for purses to the cited validator.
    pub fn reserved_purse_prefix(&self) -> Result<Vec<u8>, Error> {
        let validator = self.validator_account_hash();
        let mut ret = Vec::with_capacity(validator.serialized_length() + 2);
        ret.push(KeyTag::BidAddr as u8);
        ret.push(BidAddrTag::ReservedDelegationPurse as u8);
        validator.write_bytes(&mut ret)?;
        Ok(ret)
    }

    /// Validator account hash.
    pub fn validator_account_hash(&self) -> AccountHash {
        match self {
            BidAddr::Unified(account_hash)
            | BidAddr::Validator(account_hash)
            | BidAddr::ValidatorRev(account_hash) => *account_hash,
            BidAddr::DelegatedAccount { validator, .. }
            | BidAddr::DelegatedPurse { validator, .. }
            | BidAddr::Credit { validator, .. }
            | BidAddr::ReservedDelegationAccount { validator, .. }
            | BidAddr::ReservedDelegationPurse { validator, .. }
            | BidAddr::UnbondAccount { validator, .. }
            | BidAddr::UnbondPurse { validator, .. } => *validator,
        }
    }

    /// Delegator account hash or none.
    pub fn maybe_delegator_account_hash(&self) -> Option<AccountHash> {
        match self {
            BidAddr::Unified(_)
            | BidAddr::Validator(_)
            | BidAddr::ValidatorRev(_)
            | BidAddr::Credit { .. }
            | BidAddr::DelegatedPurse { .. }
            | BidAddr::ReservedDelegationPurse { .. }
            | BidAddr::UnbondPurse { .. } => None,
            BidAddr::DelegatedAccount { delegator, .. }
            | BidAddr::ReservedDelegationAccount { delegator, .. } => Some(*delegator),
            BidAddr::UnbondAccount { unbonder, .. } => Some(*unbonder),
        }
    }

    /// Delegator purse addr or none.
    pub fn maybe_delegator_purse(&self) -> Option<URefAddr> {
        match self {
            BidAddr::Unified(_)
            | BidAddr::Validator(_)
            | BidAddr::ValidatorRev(_)
            | BidAddr::Credit { .. }
            | BidAddr::DelegatedAccount { .. }
            | BidAddr::ReservedDelegationAccount { .. }
            | BidAddr::UnbondAccount { .. } => None,
            BidAddr::DelegatedPurse { delegator, .. }
            | BidAddr::ReservedDelegationPurse { delegator, .. } => Some(*delegator),
            BidAddr::UnbondPurse { unbonder, .. } => Some(*unbonder),
        }
    }

    /// Era id or none.
    pub fn maybe_era_id(&self) -> Option<EraId> {
        match self {
            BidAddr::Unified(_)
            | BidAddr::Validator(_)
            | BidAddr::ValidatorRev(_)
            | BidAddr::DelegatedAccount { .. }
            | BidAddr::DelegatedPurse { .. }
            | BidAddr::ReservedDelegationAccount { .. }
            | BidAddr::ReservedDelegationPurse { .. }
            | BidAddr::UnbondPurse { .. }
            | BidAddr::UnbondAccount { .. } => None,
            BidAddr::Credit { era_id, .. } => Some(*era_id),
        }
    }

    /// If true, this instance is the key for a delegator bid record.
    /// Else, it is the key for a validator bid record.
    pub fn is_delegator_bid_addr(&self) -> bool {
        match self {
            BidAddr::Unified(_)
            | BidAddr::Validator(_)
            | BidAddr::ValidatorRev(_)
            | BidAddr::Credit { .. }
            | BidAddr::ReservedDelegationAccount { .. }
            | BidAddr::ReservedDelegationPurse { .. }
            | BidAddr::UnbondPurse { .. }
            | BidAddr::UnbondAccount { .. } => false,
            BidAddr::DelegatedAccount { .. } | BidAddr::DelegatedPurse { .. } => true,
        }
    }

    /// How long will be the serialized value for this instance.
    pub fn serialized_length(&self) -> usize {
        match self {
            BidAddr::Unified(account_hash)
            | BidAddr::Validator(account_hash)
            | BidAddr::ValidatorRev(account_hash) => ToBytes::serialized_length(account_hash) + 1,
            BidAddr::DelegatedAccount {
                validator,
                delegator,
            } => ToBytes::serialized_length(validator) + ToBytes::serialized_length(delegator) + 1,
            BidAddr::DelegatedPurse {
                validator,
                delegator,
            } => ToBytes::serialized_length(validator) + ToBytes::serialized_length(delegator) + 1,
            BidAddr::Credit { validator, era_id } => {
                ToBytes::serialized_length(validator) + ToBytes::serialized_length(era_id) + 1
            }
            BidAddr::ReservedDelegationAccount {
                validator,
                delegator,
            } => ToBytes::serialized_length(validator) + ToBytes::serialized_length(delegator) + 1,
            BidAddr::ReservedDelegationPurse {
                validator,
                delegator,
            } => ToBytes::serialized_length(validator) + ToBytes::serialized_length(delegator) + 1,
            BidAddr::UnbondAccount {
                validator,
                unbonder,
            } => ToBytes::serialized_length(validator) + ToBytes::serialized_length(unbonder) + 1,
            BidAddr::UnbondPurse {
                validator,
                unbonder,
            } => ToBytes::serialized_length(validator) + ToBytes::serialized_length(unbonder) + 1,
        }
    }

    /// Returns the BiddAddrTag of this instance.
    pub fn tag(&self) -> BidAddrTag {
        match self {
            BidAddr::Unified(_) => BidAddrTag::Unified,
            BidAddr::Validator(_) => BidAddrTag::Validator,
            BidAddr::ValidatorRev(_) => BidAddrTag::ValidatorRev,
            BidAddr::DelegatedAccount { .. } => BidAddrTag::DelegatedAccount,
            BidAddr::DelegatedPurse { .. } => BidAddrTag::DelegatedPurse,

            BidAddr::Credit { .. } => BidAddrTag::Credit,
            BidAddr::ReservedDelegationAccount { .. } => BidAddrTag::ReservedDelegationAccount,
            BidAddr::ReservedDelegationPurse { .. } => BidAddrTag::ReservedDelegationPurse,
            BidAddr::UnbondAccount { .. } => BidAddrTag::UnbondAccount,
            BidAddr::UnbondPurse { .. } => BidAddrTag::UnbondPurse,
        }
    }
}

impl ToBytes for BidAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.push(self.tag() as u8);
        buffer.append(&mut self.validator_account_hash().to_bytes()?);
        if let Some(delegator) = self.maybe_delegator_purse() {
            buffer.append(&mut delegator.to_bytes()?);
        }
        if let Some(delegator) = self.maybe_delegator_account_hash() {
            buffer.append(&mut delegator.to_bytes()?);
        }
        if let Some(era_id) = self.maybe_era_id() {
            buffer.append(&mut era_id.to_bytes()?);
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.serialized_length()
    }
}

impl FromBytes for BidAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == BidAddrTag::Unified as u8 => AccountHash::from_bytes(remainder)
                .map(|(account_hash, remainder)| (BidAddr::Unified(account_hash), remainder)),
            tag if tag == BidAddrTag::Validator as u8 => AccountHash::from_bytes(remainder)
                .map(|(account_hash, remainder)| (BidAddr::Validator(account_hash), remainder)),
            tag if tag == BidAddrTag::ValidatorRev as u8 => AccountHash::from_bytes(remainder)
                .map(|(account_hash, remainder)| (BidAddr::ValidatorRev(account_hash), remainder)),
            tag if tag == BidAddrTag::DelegatedAccount as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (delegator, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((
                    BidAddr::DelegatedAccount {
                        validator,
                        delegator,
                    },
                    remainder,
                ))
            }
            tag if tag == BidAddrTag::DelegatedPurse as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (delegator, remainder) = URefAddr::from_bytes(remainder)?;
                Ok((
                    BidAddr::DelegatedPurse {
                        validator,
                        delegator,
                    },
                    remainder,
                ))
            }

            tag if tag == BidAddrTag::Credit as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (era_id, remainder) = EraId::from_bytes(remainder)?;
                Ok((BidAddr::Credit { validator, era_id }, remainder))
            }
            tag if tag == BidAddrTag::ReservedDelegationAccount as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (delegator, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((
                    BidAddr::ReservedDelegationAccount {
                        validator,
                        delegator,
                    },
                    remainder,
                ))
            }
            tag if tag == BidAddrTag::ReservedDelegationPurse as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (delegator, remainder) = URefAddr::from_bytes(remainder)?;
                Ok((
                    BidAddr::ReservedDelegationPurse {
                        validator,
                        delegator,
                    },
                    remainder,
                ))
            }
            tag if tag == BidAddrTag::UnbondAccount as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (unbonder, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((
                    BidAddr::UnbondAccount {
                        validator,
                        unbonder,
                    },
                    remainder,
                ))
            }
            tag if tag == BidAddrTag::UnbondPurse as u8 => {
                let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                let (unbonder, remainder) = URefAddr::from_bytes(remainder)?;
                Ok((
                    BidAddr::UnbondPurse {
                        validator,
                        unbonder,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Default for BidAddr {
    fn default() -> Self {
        BidAddr::Validator(AccountHash::default())
    }
}

impl From<BidAddr> for Key {
    fn from(bid_addr: BidAddr) -> Self {
        Key::BidAddr(bid_addr)
    }
}

impl From<AccountHash> for BidAddr {
    fn from(account_hash: AccountHash) -> Self {
        BidAddr::Validator(account_hash)
    }
}

impl From<PublicKey> for BidAddr {
    fn from(public_key: PublicKey) -> Self {
        BidAddr::Validator(public_key.to_account_hash())
    }
}

impl Display for BidAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let tag = self.tag();
        match self {
            BidAddr::Unified(account_hash)
            | BidAddr::Validator(account_hash)
            | BidAddr::ValidatorRev(account_hash) => {
                write!(f, "{}{}", tag, account_hash)
            }
            BidAddr::DelegatedAccount {
                validator,
                delegator,
            } => write!(f, "{}{}{}", tag, validator, delegator),
            BidAddr::DelegatedPurse {
                validator,
                delegator,
            } => write!(
                f,
                "{}{}{}",
                tag,
                validator,
                base16::encode_lower(&delegator),
            ),

            BidAddr::Credit { validator, era_id } => write!(
                f,
                "{}{}{}",
                tag,
                validator,
                base16::encode_lower(&era_id.to_le_bytes())
            ),
            BidAddr::ReservedDelegationAccount {
                validator,
                delegator,
            } => write!(f, "{}{}{}", tag, validator, delegator),
            BidAddr::ReservedDelegationPurse {
                validator,
                delegator,
            } => write!(
                f,
                "{}{}{}",
                tag,
                validator,
                base16::encode_lower(&delegator),
            ),
            BidAddr::UnbondAccount {
                validator,
                unbonder,
            } => write!(f, "{}{}{}", tag, validator, unbonder,),
            BidAddr::UnbondPurse {
                validator,
                unbonder,
            } => write!(f, "{}{}{}", tag, validator, base16::encode_lower(&unbonder),),
        }
    }
}

impl Debug for BidAddr {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        match self {
            BidAddr::Unified(validator) => write!(f, "BidAddr::Unified({:?})", validator),
            BidAddr::Validator(validator) => write!(f, "BidAddr::Validator({:?})", validator),
            BidAddr::ValidatorRev(validator) => write!(f, "BidAddr::ValidatorRev({:?})", validator),
            BidAddr::DelegatedAccount {
                validator,
                delegator,
            } => {
                write!(
                    f,
                    "BidAddr::DelegatedAccount({:?}{:?})",
                    validator, delegator
                )
            }
            BidAddr::DelegatedPurse {
                validator,
                delegator,
            } => {
                write!(f, "BidAddr::DelegatedPurse({:?}{:?})", validator, delegator)
            }
            BidAddr::Credit { validator, era_id } => {
                write!(f, "BidAddr::Credit({:?}{:?})", validator, era_id)
            }
            BidAddr::ReservedDelegationAccount {
                validator,
                delegator,
            } => {
                write!(
                    f,
                    "BidAddr::ReservedDelegationAccount({:?}{:?})",
                    validator, delegator
                )
            }
            BidAddr::ReservedDelegationPurse {
                validator,
                delegator,
            } => {
                write!(
                    f,
                    "BidAddr::ReservedDelegationPurse({:?}{:?})",
                    validator, delegator
                )
            }
            BidAddr::UnbondAccount {
                validator,
                unbonder,
            } => {
                write!(f, "BidAddr::UnbondAccount({:?}{:?})", validator, unbonder)
            }
            BidAddr::UnbondPurse {
                validator,
                unbonder,
            } => {
                write!(f, "BidAddr::UnbondPurse({:?}{:?})", validator, unbonder)
            }
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<BidAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> BidAddr {
        BidAddr::Validator(AccountHash::new(rng.gen()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, system::auction::BidAddr, EraId, PublicKey, SecretKey};

    #[test]
    fn serialization_roundtrip() {
        let bid_addr = BidAddr::legacy([1; 32]);
        bytesrepr::test_serialization_roundtrip(&bid_addr);
        let bid_addr = BidAddr::new_validator_addr([1; 32]);
        bytesrepr::test_serialization_roundtrip(&bid_addr);
        let bid_addr = BidAddr::new_delegator_account_addr(([1; 32], [2; 32]));
        bytesrepr::test_serialization_roundtrip(&bid_addr);
        let bid_addr = BidAddr::new_credit(
            &PublicKey::from(
                &SecretKey::ed25519_from_bytes([0u8; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            EraId::new(0),
        );
        bytesrepr::test_serialization_roundtrip(&bid_addr);
        let bid_addr = BidAddr::new_reservation_account_addr(([1; 32], [2; 32]));
        bytesrepr::test_serialization_roundtrip(&bid_addr);
    }
}

#[cfg(test)]
mod proptest {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid_addr_validator(bid_addr in gens::bid_addr_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid_addr);
        }
    }
}
