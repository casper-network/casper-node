pub(crate) mod runtime_provider;
pub(crate) mod storage_provider;
pub(crate) mod system_provider;

use num_rational::Ratio;
use num_traits::CheckedMul;

use casper_types::{
    account::AccountHash,
    system::{
        mint::{Error, ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY},
        CallStackElement,
    },
    Key, Phase, PublicKey, URef, U512,
};

use crate::system::mint::{
    runtime_provider::RuntimeProvider, storage_provider::StorageProvider,
    system_provider::SystemProvider,
};

/// Mint trait.
pub trait Mint: RuntimeProvider + StorageProvider + SystemProvider {
    /// Mint new token with given `initial_balance` balance. Returns new purse on success, otherwise
    /// an error.
    fn mint(&mut self, initial_balance: U512) -> Result<URef, Error> {
        let caller = self.get_caller();
        let is_empty_purse = initial_balance.is_zero();
        if !is_empty_purse && caller != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidNonEmptyPurseCreation);
        }

        let purse_uref: URef = self.new_uref(())?;
        self.write_balance(purse_uref, initial_balance)?;

        if !is_empty_purse {
            // get total supply uref if exists, otherwise error
            let total_supply_uref = match self.get_key(TOTAL_SUPPLY_KEY) {
                None => {
                    // total supply URef should exist due to genesis
                    return Err(Error::TotalSupplyNotFound);
                }
                Some(Key::URef(uref)) => uref,
                Some(_) => return Err(Error::MissingKey),
            };
            // increase total supply
            self.add(total_supply_uref, initial_balance)?;
        }

        Ok(purse_uref)
    }

    /// Reduce total supply by `amount`. Returns unit on success, otherwise
    /// an error.
    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error> {
        // only system may reduce total supply
        let caller = self.get_caller();
        if caller != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidTotalSupplyReductionAttempt);
        }

        if amount.is_zero() {
            return Ok(()); // no change to supply
        }

        // get total supply or error
        let total_supply_uref = match self.get_key(TOTAL_SUPPLY_KEY) {
            Some(Key::URef(uref)) => uref,
            Some(_) => return Err(Error::MissingKey), // TODO
            None => return Err(Error::MissingKey),
        };
        let total_supply: U512 = self
            .read(total_supply_uref)?
            .ok_or(Error::TotalSupplyNotFound)?;

        // decrease total supply
        let reduced_total_supply = total_supply
            .checked_sub(amount)
            .ok_or(Error::ArithmeticOverflow)?;

        // update total supply
        self.write(total_supply_uref, reduced_total_supply)?;

        Ok(())
    }

    /// Read balance of given `purse`.
    fn balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        match self.read_balance(purse)? {
            some @ Some(_) => Ok(some),
            None => Err(Error::PurseNotFound),
        }
    }

    /// Transfers `amount` of tokens from `source` purse to a `target` purse.
    fn transfer(
        &mut self,
        maybe_to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), Error> {
        if let (Phase::Session, Some(&CallStackElement::StoredSession { .. })) =
            (self.get_phase(), self.get_immediate_caller())
        {
            // stored session code is not allowed to call this method in the session phase
            return Err(Error::InvalidContext);
        }

        if !source.is_readable() {
            return Err(Error::InvalidAccessRights);
        }
        if !source.is_writeable() || !target.is_addable() {
            return Err(Error::InvalidAccessRights);
        }
        let source_balance: U512 = match self.read_balance(source)? {
            Some(source_balance) => source_balance,
            None => return Err(Error::SourceNotFound),
        };
        if amount > source_balance {
            return Err(Error::InsufficientFunds);
        }

        if self.read_balance(target)?.is_none() {
            return Err(Error::DestNotFound);
        }

        if self.get_main_purse().addr() == source.addr() {
            if amount > self.get_approved_spending_limit() {
                return Err(Error::UnapprovedSpendingAmount);
            }
            self.sub_approved_spending_limit(amount);
        }

        self.write_balance(source, source_balance - amount)?;
        self.add_balance(target, amount)?;

        self.record_transfer(maybe_to, source, target, amount, id)?;
        Ok(())
    }

    /// Retrieves the base round reward.
    fn read_base_round_reward(&mut self) -> Result<U512, Error> {
        let total_supply_uref = match self.get_key(TOTAL_SUPPLY_KEY) {
            Some(Key::URef(uref)) => uref,
            Some(_) => return Err(Error::MissingKey), // TODO
            None => return Err(Error::MissingKey),
        };
        let total_supply: U512 = self
            .read(total_supply_uref)?
            .ok_or(Error::TotalSupplyNotFound)?;

        let round_seigniorage_rate_uref = match self.get_key(ROUND_SEIGNIORAGE_RATE_KEY) {
            Some(Key::URef(uref)) => uref,
            Some(_) => return Err(Error::MissingKey), // TODO
            None => return Err(Error::MissingKey),
        };
        let round_seigniorage_rate: Ratio<U512> = self
            .read(round_seigniorage_rate_uref)?
            .ok_or(Error::TotalSupplyNotFound)?;

        round_seigniorage_rate
            .checked_mul(&Ratio::from(total_supply))
            .map(|ratio| ratio.to_integer())
            .ok_or(Error::ArithmeticOverflow)
    }

    /// Mint `amount` new token into `existing_purse`.
    /// Returns unit on success, otherwise an error.
    fn mint_into_existing_purse(
        &mut self,
        existing_purse: URef,
        amount: U512,
    ) -> Result<(), Error> {
        let caller = self.get_caller();
        if caller != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidContext);
        }
        if amount.is_zero() {
            // treat as noop
            return Ok(());
        }
        if self.read_balance(existing_purse)?.is_none() {
            return Err(Error::PurseNotFound);
        }
        self.add_balance(existing_purse, amount)?;
        // get total supply uref if exists, otherwise error.
        let total_supply_uref = match self.get_key(TOTAL_SUPPLY_KEY) {
            None => {
                // total supply URef should exist due to genesis
                // which obviously must have been called
                // before new rewards are minted at the end of an era
                return Err(Error::TotalSupplyNotFound);
            }
            Some(Key::URef(uref)) => uref,
            Some(_) => return Err(Error::MissingKey),
        };
        // increase total supply
        self.add(total_supply_uref, amount)?;
        Ok(())
    }
}
