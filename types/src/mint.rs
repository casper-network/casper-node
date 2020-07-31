//! Contains implementation of a Mint contract functionality.
mod runtime_provider;
mod storage_provider;

use alloc::{collections::BTreeMap, vec::Vec};
use core::convert::TryFrom;

use crate::{account::AccountHash, system_contract_errors::mint::Error, Key, URef, U512};

pub use crate::mint::{runtime_provider::RuntimeProvider, storage_provider::StorageProvider};

const SYSTEM_ACCOUNT: AccountHash = AccountHash::new([0; 32]);

/// Bidders mapped to their bidding purses and tokens contained therein. Delegators' tokens
/// are kept in the validator bid purses, available for withdrawal up to the delegated number
/// of tokens. Withdrawal moves the tokens to a delegator-controlled unbonding purse.
pub type BidPurses = BTreeMap<AccountHash, (URef, U512)>;

/// Founding validators mapped to their staking purses and tokens contained therein. These
/// function much like the regular bidding purses, but have a field indicating whether any tokens
/// may be unbonded.
pub type FounderPurses = BTreeMap<AccountHash, (URef, U512, bool)>;

/// Validators and delegators mapped to their purses, tokens and expiration timer in eras. At the
/// beginning of each era, node software updates the timer until it reaches 0, at which point
/// tokens may be transferred to a different purse.
pub type UnbondingPurses = BTreeMap<AccountHash, (AccountHash, U512, u8)>;

/// Mint trait.
pub trait Mint: RuntimeProvider + StorageProvider {
    /// Mint new token with given `initial_balance` balance. Returns new purse on success, otherwise
    /// an error.
    fn mint(&mut self, initial_balance: U512) -> Result<URef, Error> {
        let caller = self.get_caller();
        if !initial_balance.is_zero() && caller != SYSTEM_ACCOUNT {
            return Err(Error::InvalidNonEmptyPurseCreation);
        }

        let balance_key: Key = self.new_uref(initial_balance).into();
        let purse_uref: URef = self.new_uref(());
        let purse_uref_name = purse_uref.remove_access_rights().to_formatted_string();

        // store balance uref so that the runtime knows the mint has full access
        self.put_key(&purse_uref_name, balance_key);

        // store association between purse id and balance uref
        self.write_local(purse_uref.addr(), balance_key);
        // self.write(purse_uref.addr(), Key::Hash)

        Ok(purse_uref)
    }

    /// Read balance of given `purse`.
    fn balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        let balance_uref: URef = match self.read_local(&purse.addr())? {
            Some(key) => TryFrom::<Key>::try_from(key).map_err(|_| Error::InvalidAccessRights)?,
            None => return Ok(None),
        };
        match self.read(balance_uref)? {
            some @ Some(_) => Ok(some),
            None => Err(Error::PurseNotFound),
        }
    }

    /// Transfers `amount` of tokens from `source` purse to a `target` purse.
    fn transfer(&mut self, source: URef, target: URef, amount: U512) -> Result<(), Error> {
        if !source.is_writeable() || !target.is_addable() {
            return Err(Error::InvalidAccessRights);
        }
        let source_balance: URef = match self.read_local(&source.addr())? {
            Some(key) => TryFrom::<Key>::try_from(key).map_err(|_| Error::InvalidAccessRights)?,
            None => return Err(Error::SourceNotFound),
        };
        let source_value: U512 = match self.read(source_balance)? {
            Some(source_value) => source_value,
            None => return Err(Error::SourceNotFound),
        };
        if amount > source_value {
            return Err(Error::InsufficientFunds);
        }
        let target_balance: URef = match self.read_local(&target.addr())? {
            Some(key) => TryFrom::<Key>::try_from(key).map_err(|_| Error::InvalidAccessRights)?,
            None => return Err(Error::DestNotFound),
        };
        self.write(source_balance, source_value - amount)?;
        self.add(target_balance, amount)?;
        Ok(())
    }

    /// Creates a new purse in bid_purses corresponding to a validator’s key, or tops off an
    /// existing one.
    ///
    /// Returns the bid purse's key and current quantity of motes.
    fn bond(
        &mut self,
        _account_hash: AccountHash,
        _source_purse: URef,
        _quantity: U512,
    ) -> Result<(URef, U512), Error> {
        let bid_purses_uref = self
            .get_key("bid_purses")
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let mut _bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;
        // bid_purses.entry(&account_hash).

        todo!("bond")
    }

    /// Creates a new purse in unbonding_purses given a validator's key and quantity, returning the new purse's key and the quantity of motes remaining in the validator's bid purse.
    fn unbond(
        &mut self,
        _account_hash: AccountHash,
        _source_purse: URef,
        _quantity: U512,
    ) -> Result<(URef, U512), Error> {
        let bid_purses_uref = self
            .get_key("bid_purses")
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let mut _bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;

        todo!("unbond")
    }

    /// In the first block of each era, the node submits a special deploy that calls this function,
    /// decrementing the number of eras until unlock for every value in unbonding_purses.
    fn unbond_timer_advance(&mut self) -> Result<(), Error> {
        todo!("unbond_timer_advance");
    }

    /// In the first block of each era, the node submits a special deploy that calls this function, decrementing the number of eras until unlock for every value in unbonding_purses.
    fn slash(&mut self, _validator_account_hashes: Vec<AccountHash>) -> Result<(), Error> {
        // Present version of this document does not specify how unbonding delegators are to be
        // slashed (this will require some modifications to the spec).
        todo!("slash")
    }

    /// Sets the Bool field in the tuple representing a founding validator’s stake to True,
    /// enabling this validator to unbond.
    fn release_founder_stake(
        &mut self,
        _validator_account_hash: AccountHash,
    ) -> Result<bool, Error> {
        todo!("release_founder_stake")
    }
}
