use num::{CheckedMul, One};
use num_rational::Ratio;
use tracing::error;

use casper_types::{
    account::AccountHash,
    system::handle_payment::{Error, ACCUMULATION_PURSE_KEY, PAYMENT_PURSE_KEY, REFUND_PURSE_KEY},
    FeeHandling, Key, Phase, PublicKey, RefundHandling, URef, U512,
};

use super::{
    mint_provider::MintProvider, runtime_provider::RuntimeProvider,
    storage_provider::StorageProvider,
};

/// Returns the purse for accepting payment for transactions.
pub fn get_payment_purse<R: RuntimeProvider>(runtime_provider: &mut R) -> Result<URef, Error> {
    match runtime_provider.get_key(PAYMENT_PURSE_KEY) {
        Some(Key::URef(uref)) => Ok(uref),
        Some(_) => Err(Error::PaymentPurseKeyUnexpectedType),
        None => Err(Error::PaymentPurseNotFound),
    }
}

/// Sets the purse where refunds (excess funds not spent to pay for computation) will be sent.
/// Note that if this function is never called, the default location is the main purse of the
/// deployer's account.
pub fn set_refund<R: RuntimeProvider>(runtime_provider: &mut R, purse: URef) -> Result<(), Error> {
    if let Phase::Payment = runtime_provider.get_phase() {
        runtime_provider.put_key(REFUND_PURSE_KEY, Key::URef(purse))?;
        return Ok(());
    }
    Err(Error::SetRefundPurseCalledOutsidePayment)
}

/// Returns the currently set refund purse.
pub fn get_refund_purse<R: RuntimeProvider>(
    runtime_provider: &mut R,
) -> Result<Option<URef>, Error> {
    match runtime_provider.get_key(REFUND_PURSE_KEY) {
        Some(Key::URef(uref)) => Ok(Some(uref)),
        Some(_) => Err(Error::RefundPurseKeyUnexpectedType),
        None => Ok(None),
    }
}

/// Returns tuple where 1st element is the refund, and 2nd element is the fee.
///
/// # Note
///
/// Any dust amounts are added to the fee.
fn calculate_refund_and_fee(
    gas_spent: U512,
    payment_purse_balance: U512,
    refund_handling: &RefundHandling,
) -> Result<(U512, U512), Error> {
    let unspent = payment_purse_balance
        .checked_sub(gas_spent)
        .ok_or(Error::ArithmeticOverflow)?;

    let refund_ratio = match refund_handling {
        RefundHandling::Refund { refund_ratio } | RefundHandling::Burn { refund_ratio } => {
            debug_assert!(
                refund_ratio <= &Ratio::one(),
                "refund ratio should be a proper fraction"
            );
            let (numer, denom) = (*refund_ratio).into();
            Ratio::new_raw(U512::from(numer), U512::from(denom))
        }
    };

    let refund = Ratio::from(unspent)
        .checked_mul(&refund_ratio)
        .ok_or(Error::ArithmeticOverflow)?
        .to_integer();

    let fee = payment_purse_balance
        .checked_sub(refund)
        .ok_or(Error::ArithmeticOverflow)?;

    Ok((refund, fee))
}

/// Transfers funds from the payment purse to the proposer, accumulation purse or burns the amount
/// depending on a [`FeeHandling`] configuration option. This function can also transfer funds to a
/// refund purse, depending on how much was spent on the computation, or burns the refund. This code
/// maintains the invariant that the balance of the payment purse is zero at the beginning and end
/// of each deploy and that the refund purse is unset at the beginning and end of each deploy.
pub fn finalize_payment<P: MintProvider + RuntimeProvider + StorageProvider>(
    provider: &mut P,
    gas_spent: U512,
    account: AccountHash,
    target: URef,
) -> Result<(), Error> {
    let caller = provider.get_caller();
    if caller != PublicKey::System.to_account_hash() {
        return Err(Error::SystemFunctionCalledByUserAccount);
    }

    let payment_purse = get_payment_purse(provider)?;
    let mut payment_amount = match provider.balance(payment_purse)? {
        Some(balance) => balance,
        None => return Err(Error::PaymentPurseBalanceNotFound),
    };

    if payment_amount < gas_spent {
        return Err(Error::InsufficientPaymentForAmountSpent);
    }

    let (refund, fee) =
        calculate_refund_and_fee(gas_spent, payment_amount, provider.refund_handling())?;

    debug_assert_eq!(fee + refund, payment_amount);

    // Give or burn the refund.
    match provider.refund_handling() {
        RefundHandling::Refund { .. } => {
            let refund_purse = get_refund_purse(provider)?;

            if let Some(refund_purse) = refund_purse {
                if refund_purse.remove_access_rights() == payment_purse.remove_access_rights() {
                    // Make sure we're not refunding into a payment purse to invalidate payment
                    // code postconditions.
                    return Err(Error::RefundPurseIsPaymentPurse);
                }
            }

            provider.remove_key(REFUND_PURSE_KEY)?; //unset refund purse after reading it

            if !refund.is_zero() {
                match refund_purse {
                    Some(refund_purse) => {
                        // In case of failure to transfer to refund purse we fall back on the
                        // account's main purse
                        match provider.transfer_purse_to_purse(payment_purse, refund_purse, refund)
                        {
                            Ok(()) => {}
                            Err(error) => {
                                error!(
                                    %error,
                                    %refund,
                                    %account,
                                    "unable to transfer refund to a refund purse; refunding to account"
                                );
                                refund_to_account::<P>(provider, payment_purse, account, refund)?;
                            }
                        }
                    }
                    None => {
                        refund_to_account::<P>(provider, payment_purse, account, refund)?;
                    }
                }
            }
        }

        RefundHandling::Burn { .. } if !refund.is_zero() => {
            // Fee-handling is set to `Burn`.  Deduct the refund from the payment purse and
            // reduce the total supply (i.e. burn the refund).
            payment_amount = payment_amount
                .checked_sub(refund)
                .ok_or(Error::ArithmeticOverflow)?;

            provider.write_balance(payment_purse, payment_amount)?;
            provider.reduce_total_supply(refund)?;
        }

        RefundHandling::Burn { .. } => {
            // No refund to burn
        }
    }

    // Pay or burn the fee.
    match provider.fee_handling() {
        FeeHandling::PayToProposer | FeeHandling::Accumulate => {
            // target purse is already resolved based on fee-handling config which is either a
            // proposer or accumulation purse.
            match provider.transfer_purse_to_purse(payment_purse, target, fee) {
                Ok(()) => {}
                Err(error) => {
                    error!(%error, %fee, %target, "unable to transfer fee");
                    return Err(Error::FailedTransferToRewardsPurse);
                }
            }
        }
        FeeHandling::Burn => {
            debug_assert_eq!(
                target,
                URef::default(),
                "Caller should pass a defaulted URef if the fees are burned"
            );
            // Fee-handling is set to `Burn`.  Deduct the fee from the payment purse, leaving it
            // empty, and reduce the total supply (i.e. burn the fee).
            provider.write_balance(payment_purse, U512::zero())?;
            provider.reduce_total_supply(fee)?;
        }
    }
    Ok(())
}

pub fn refund_to_account<M: MintProvider>(
    mint_provider: &mut M,
    payment_purse: URef,
    account: AccountHash,
    amount: U512,
) -> Result<(), Error> {
    match mint_provider.transfer_purse_to_account(payment_purse, account, amount) {
        Ok(_) => Ok(()),
        Err(error) => {
            error!(%error, %amount, %account, "unable to process refund from payment purse to account");
            Err(Error::FailedTransferToAccountPurse)
        }
    }
}

/// Gets an accumulation purse from the named keys.
fn get_accumulation_purse<R: RuntimeProvider>(provider: &mut R) -> Result<URef, Error> {
    match provider.get_key(ACCUMULATION_PURSE_KEY) {
        Some(Key::URef(purse_uref)) => Ok(purse_uref),
        Some(_key) => Err(Error::AccumulationPurseKeyUnexpectedType),
        None => Err(Error::AccumulationPurseNotFound),
    }
}

/// This function distributes the fees according to the fee handling config.
pub fn distribute_accumulated_fees<P>(provider: &mut P) -> Result<(), Error>
where
    P: RuntimeProvider + MintProvider,
{
    if provider.get_caller() != PublicKey::System.to_account_hash() {
        return Err(Error::SystemFunctionCalledByUserAccount);
    }

    // Distribute accumulation purse balance into all administrators
    match provider.fee_handling() {
        FeeHandling::PayToProposer | FeeHandling::Burn => return Ok(()),
        FeeHandling::Accumulate => {}
    }

    let administrative_accounts = provider.administrative_accounts().clone();
    let accumulation_purse = get_accumulation_purse(provider)?;
    let accumulated_balance = provider.balance(accumulation_purse)?.unwrap_or_default();
    let reward_recipients = U512::from(administrative_accounts.len());

    if let Some(reward_amount) = accumulated_balance.checked_div(reward_recipients) {
        if reward_amount.is_zero() {
            // There is zero tokens to be paid out which means we can exit early.
            return Ok(());
        }

        for target in administrative_accounts {
            provider.transfer_purse_to_account(accumulation_purse, target, reward_amount)?;
        }
    }

    // Any dust amount left in the accumulation purse for the next round.

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_move_dust_to_reward() {
        let refund_ratio = Ratio::new_raw(1, 3);
        let refund = RefundHandling::Refund { refund_ratio };
        test_refund_handling(&refund);

        let burn = RefundHandling::Burn { refund_ratio };
        test_refund_handling(&burn);
    }

    fn test_refund_handling(refund_handling: &RefundHandling) {
        let purse_bal = U512::from(10u64);
        let gas = U512::from(3u64);
        let (a, b) = calculate_refund_and_fee(gas, purse_bal, refund_handling).unwrap();
        assert_eq!(a, U512::from(2u64));
        // (10 - 3) * 1/3 ~ 2.33 (.33 is dust)
        assert_eq!(b, U512::from(8u64));
        // 10 - 2 = 8
    }

    #[test]
    fn should_account_refund_for_dust() {
        let purse_bal = U512::from(9973u64);
        let gas = U512::from(9161u64);

        for percentage in 0..=100 {
            let refund_ratio = Ratio::new_raw(percentage, 100);
            let refund = RefundHandling::Refund { refund_ratio };

            let (a, b) = calculate_refund_and_fee(gas, purse_bal, &refund).unwrap();

            let a = Ratio::from(a);
            let b = Ratio::from(b);

            assert_eq!(a + b, Ratio::from(purse_bal));
        }
    }
}

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use super::*;

    const DENOM_MAX: u64 = 1000;
    const BALANCE_MAX: u64 = 100_000_000;

    prop_compose! {
      fn proper_fraction(max: u64)
                        (numerator in 0..=max)
                        (numerator in Just(numerator), denom in numerator..=max) -> Ratio<u64> {
        Ratio::new(numerator, denom)
      }
    }

    prop_compose! {
      fn balance_and_gas(max_balance: u64)(balance in 100..=max_balance)(balance in Just(balance), gas in 1..=balance) -> (U512, U512) {
        (U512::from(balance), U512::from(gas))
      }
    }

    proptest! {
        #[test]
        fn refund_and_fee_equals_balance(refund_ratio in proper_fraction(DENOM_MAX), (balance, gas) in balance_and_gas(BALANCE_MAX)) {
            let refund = RefundHandling::Refund { refund_ratio };

            let (refund, fee) = calculate_refund_and_fee(gas, balance, &refund).unwrap();
            prop_assert_eq!(refund + fee, balance);
        }
    }
}
