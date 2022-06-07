pub(crate) mod runtime_provider;
pub(crate) mod storage_provider;
pub(crate) mod system_provider;

use num_rational::Ratio;
use num_traits::CheckedMul;
use tracing::warn;

use casper_types::{
    account::AccountHash,
    system::{
        mint::{Error, ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY},
        CallStackElement,
    },
    Key, Phase, PublicKey, StoredValue, URef, U512,
};

use crate::{
    core::engine_state::SystemContractRegistry,
    system::mint::{
        runtime_provider::RuntimeProvider, storage_provider::StorageProvider,
        system_provider::SystemProvider,
    },
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
        if let (Phase::Session, Some(CallStackElement::StoredSession { .. })) =
            (self.get_phase(), self.get_immediate_caller())
        {
            // stored session code is not allowed to call this method in the session phase
            return Err(Error::InvalidContext);
        }

        if !self.allow_unrestricted_transfers() {
            let registry = match self.get_system_contract_registry() {
                Ok(registry) => registry,
                Err(error) => {
                    warn!(%error, "unable to obtain system contract registry during transfer");
                    SystemContractRegistry::new()
                }
            };
            let immediate_caller = self.get_immediate_caller().cloned();
            match immediate_caller {
                Some(CallStackElement::StoredSession { contract_hash, .. })
                | Some(CallStackElement::StoredContract { contract_hash, .. })
                    if registry.has_contract_hash(&contract_hash) =>
                {
                    // System contract calling a mint is fine (i.e. standard payment calling mint's
                    // transfer)
                }

                Some(CallStackElement::StoredSession {
                    account_hash,
                    contract_package_hash: _,
                    contract_hash: _,
                }) => {
                    if account_hash != PublicKey::System.to_account_hash() {
                        if let Some(is_account_admin) = self.is_account_administrator(&account_hash)
                        {
                            if !is_account_admin {
                                return Err(Error::DisabledUnrestrictedTransfers);
                            }
                        }
                    }
                }

                Some(CallStackElement::Session { account_hash: _ })
                    if self.is_called_from_standard_payment() =>
                {
                    // Standard payment acts as a session without separate stack frame and calls
                    // into mint's transfer.
                }

                Some(CallStackElement::Session { account_hash })
                    if account_hash == PublicKey::System.to_account_hash() =>
                {
                    // System calls a session code.
                }

                Some(CallStackElement::Session { account_hash }) => {
                    // For example: a session using transfer host functions, or calling the mint's
                    // entrypoint directly

                    if let Some(is_source_admin) = self.is_account_administrator(&account_hash) {
                        match maybe_to {
                            Some(to) => {
                                let maybe_account = self.read_account(&to);

                                match maybe_account {
                                    Ok(Some(StoredValue::Account(account))) => {
                                        // This can happen when user tries to transfer funds by
                                        // calling mint
                                        // directly but tries to specify wrong account hash.
                                        if account.main_purse().addr() != target.addr() {
                                            return Err(Error::DisabledUnrestrictedTransfers);
                                        }
                                        // SAFETY: is_account_administrator returns Some earlier
                                        // in the flow.
                                        let is_target_admin = self
                                            .is_account_administrator(&account.account_hash())
                                            .expect(
                                                "is_account_administrator return Some in earlier call",
                                            );
                                        if !is_source_admin && !is_target_admin {
                                            return Err(Error::DisabledUnrestrictedTransfers);
                                        }
                                    }
                                    Ok(Some(_stored_value)) => {
                                        return Err(Error::DisabledUnrestrictedTransfers);
                                    }
                                    Ok(None) => {
                                        // `to` is specified, but no new account is persisted
                                        // yet. Only
                                        // administrators can do that and it is also validated
                                        // at the host function level.
                                        if !is_source_admin {
                                            return Err(Error::DisabledUnrestrictedTransfers);
                                        }
                                    }
                                    Err(error) => {
                                        warn!(%error, "error while reading account");
                                        return Err(Error::Storage);
                                    }
                                }
                            }
                            None => {
                                if !is_source_admin {
                                    return Err(Error::DisabledUnrestrictedTransfers);
                                }
                            }
                        }
                    }
                }

                Some(CallStackElement::StoredContract {
                    contract_package_hash: _,
                    contract_hash: _,
                }) => {
                    if self.get_caller() != PublicKey::System.to_account_hash() {
                        if let Some(is_account_admin) =
                            self.is_account_administrator(&self.get_caller())
                        {
                            if !is_account_admin {
                                return Err(Error::DisabledUnrestrictedTransfers);
                            }
                        }
                    }
                }

                None => {
                    // There's always an immediate caller, but we should return something.
                    return Err(Error::DisabledUnrestrictedTransfers);
                }
            }
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
        if self.get_caller() != PublicKey::System.to_account_hash()
            && self.get_main_purse().addr() == source.addr()
        {
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
