//! Contains implementation of a Mint contract functionality.
mod era_provider;
mod runtime_provider;
mod storage_provider;
mod unbonding_purse;

use alloc::{collections::BTreeMap, vec::Vec};
use core::convert::TryFrom;

use crate::{
    account::AccountHash, system_contract_errors::mint::Error, Key, PublicKey, URef, U512,
};

pub use crate::mint::{
    era_provider::EraProvider, runtime_provider::RuntimeProvider,
    storage_provider::StorageProvider, unbonding_purse::UnbondingPurses,
};
use unbonding_purse::UnbondingPurse;

const SYSTEM_ACCOUNT: AccountHash = AccountHash::new([0; 32]);

/// Bidders mapped to their bidding purses and tokens contained therein. Delegators' tokens
/// are kept in the validator bid purses, available for withdrawal up to the delegated number
/// of tokens. Withdrawal moves the tokens to a delegator-controlled unbonding purse.
pub type BidPurses = BTreeMap<PublicKey, URef>;

/// Founding validators mapped to their staking purses and tokens contained therein. These
/// function much like the regular bidding purses, but have a field indicating whether any tokens
/// may be unbonded.
pub type FounderPurses = BTreeMap<AccountHash, (URef, bool)>;

/// Name of bid purses named key.
pub const BID_PURSES_KEY: &str = "bid_purses";
/// Name of founder purses named key.
pub const FOUNDER_PURSES_KEY: &str = "founder_purses";
/// Name of unbonding purses key.
pub const UNBONDING_PURSES_KEY: &str = "unbonding_purses";
const _REWARD_PURSES: &str = "reward_purses";

const DEFAULT_LOCK_IN_DURATION: u8 = 10;

/// Default number of eras that need to pass to be able to withdraw unbonded funds.
pub const DEFAULT_UNBONDING_DELAY: u16 = 14;

/// Mint trait.
pub trait Mint: RuntimeProvider + StorageProvider + EraProvider {
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
        public_key: PublicKey,
        source_purse: URef,
        quantity: U512,
    ) -> Result<(URef, U512), Error> {
        let bid_purses_uref = self
            .get_key(BID_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let mut bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;

        let bond_purse = match bid_purses.get(&public_key) {
            Some(purse) => *purse,
            None => {
                let new_purse = self.mint(U512::zero())?;
                bid_purses.insert(public_key, new_purse);
                self.write(bid_purses_uref, bid_purses)?;
                new_purse
            }
        };

        self.transfer(source_purse, bond_purse, quantity)?;

        let total_amount = self.balance(bond_purse)?.unwrap();

        Ok((bond_purse, total_amount))
    }

    /// Creates a new purse in unbonding_purses given a validator's key and quantity, returning the new purse's key and the quantity of motes remaining in the validator's bid purse.
    fn unbond(&mut self, public_key: PublicKey, quantity: U512) -> Result<(URef, U512), Error> {
        let bid_purses_uref = self
            .get_key(BID_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;

        let bid_purse = bid_purses
            .get(&public_key)
            .copied()
            .ok_or(Error::BondNotFound)?;
        // Creates new unbonding purse with requested tokens
        let unbond_purse = self.mint(U512::zero())?;
        self.transfer(bid_purse, unbond_purse, quantity)?;

        // Update `unbonding_purses` data
        let unbonding_purses_uref = self
            .get_key(UNBONDING_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;
        let mut unbonding_purses: UnbondingPurses =
            self.read(unbonding_purses_uref)?.ok_or(Error::Storage)?;
        let new_unbonding_purse = UnbondingPurse {
            purse: unbond_purse,
            origin: public_key,
            era_of_withdrawal: DEFAULT_UNBONDING_DELAY,
            expiration_timer: DEFAULT_LOCK_IN_DURATION,
        };
        unbonding_purses
            .entry(public_key)
            .and_modify(|unbonding_list| unbonding_list.push(new_unbonding_purse))
            .or_insert_with(|| [new_unbonding_purse].to_vec());
        self.write(unbonding_purses_uref, unbonding_purses)?;

        // Remaining motes in the validator's bid purse
        let remaining_bond = self.balance(bid_purse)?.unwrap();
        Ok((unbond_purse, remaining_bond))
    }

    /// In the first block of each era, the node submits a special deploy that calls this function,
    /// decrementing the number of eras until unlock for every value in unbonding_purses.
    fn unbond_timer_advance(&mut self) -> Result<(), Error> {
        // Update `unbonding_purses` data
        let unbonding_purses_uref = self
            .get_key(UNBONDING_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;
        let mut unbonding_purses: UnbondingPurses =
            self.read(unbonding_purses_uref)?.ok_or(Error::Storage)?;
        for unbonding_list in unbonding_purses.values_mut() {
            for unbonding_purse in unbonding_list {
                if unbonding_purse.expiration_timer > 0 {
                    // Advance timer for each unbond in the list
                    unbonding_purse.expiration_timer -= 1;
                }
                // Expiration timer == 0 -> tokens are unlocked
            }
        }
        self.write(unbonding_purses_uref, unbonding_purses)?;
        Ok(())
    }

    /// In the first block of each era, the node submits a special deploy that calls this function, decrementing the number of eras until unlock for every value in unbonding_purses.
    fn slash(&mut self, validator_public_keys: Vec<PublicKey>) -> Result<(), Error> {
        // Present version of this document does not specify how unbonding delegators are to be
        // slashed (this will require some modifications to the spec).
        let bid_purses_uref = self
            .get_key(BID_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;

        let mut bid_purses: BidPurses = self.read(bid_purses_uref)?.ok_or(Error::Storage)?;

        let unbonding_purses_uref = self
            .get_key(UNBONDING_PURSES_KEY)
            .and_then(Key::into_uref)
            .ok_or(Error::MissingKey)?;
        let mut unbonding_purses: UnbondingPurses =
            self.read(unbonding_purses_uref)?.ok_or(Error::Storage)?;

        let mut bid_purses_modified = false;
        let mut unbonding_purses_modified = false;
        for validator_account_hash in validator_public_keys {
            if let Some(_bid_purse) = bid_purses.remove(&validator_account_hash) {
                bid_purses_modified = true;
            }

            if let Some(unbonding_list) = unbonding_purses.get_mut(&validator_account_hash) {
                let size_before = unbonding_list.len();

                unbonding_list.retain(|element| element.origin != validator_account_hash);

                unbonding_purses_modified = size_before != unbonding_list.len();
            }
        }

        if bid_purses_modified {
            self.write(bid_purses_uref, bid_purses)?;
        }

        if unbonding_purses_modified {
            self.write(unbonding_purses_uref, unbonding_purses)?;
        }

        Ok(())
    }
}
