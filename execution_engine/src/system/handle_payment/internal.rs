use num::{CheckedMul, One};
use num_rational::Ratio;
use tracing::error;

use casper_types::{
    account::AccountHash,
    system::handle_payment::{Error, PAYMENT_PURSE_KEY, REFUND_PURSE_KEY},
    Key, Phase, PublicKey, URef, U512,
};

use super::{mint_provider::MintProvider, runtime_provider::RuntimeProvider};
use crate::core::engine_state::engine_config::RefundHandling;

/// Returns the purse for accepting payment for transactions.
pub(crate) fn get_payment_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<URef, Error> {
    match runtime_provider.get_key(PAYMENT_PURSE_KEY) {
        Some(Key::URef(uref)) => Ok(uref),
        Some(_) => Err(Error::PaymentPurseKeyUnexpectedType),
        None => Err(Error::PaymentPurseNotFound),
    }
}

/// Sets the purse where refunds (excess funds not spent to pay for computation) will be sent.
/// Note that if this function is never called, the default location is the main purse of the
/// deployer's account.
pub(crate) fn set_refund<R: RuntimeProvider>(
    runtime_provider: &mut R,
    purse: URef,
) -> Result<(), Error> {
    if let Phase::Payment = runtime_provider.get_phase() {
        runtime_provider.put_key(REFUND_PURSE_KEY, Key::URef(purse))?;
        return Ok(());
    }
    Err(Error::SetRefundPurseCalledOutsidePayment)
}

/// Returns the currently set refund purse.
pub(crate) fn get_refund_purse<R: RuntimeProvider>(
    runtime_provider: &R,
) -> Result<Option<URef>, Error> {
    match runtime_provider.get_key(REFUND_PURSE_KEY) {
        Some(Key::URef(uref)) => Ok(Some(uref)),
        Some(_) => Err(Error::RefundPurseKeyUnexpectedType),
        None => Ok(None),
    }
}

/// Returns tuple where 1st element is the refund, and 2nd element is the reward.
fn calculate_refund_and_reward(
    gas_spent: U512,
    payment_purse_balance: U512,
    refund_handling: &RefundHandling,
) -> Result<(U512, U512), Error> {
    let unspent = payment_purse_balance
        .checked_sub(gas_spent)
        .ok_or(Error::ArithmeticOverflow)?;

    let refund_ratio = match refund_handling {
        RefundHandling::Refund { refund_ratio } => {
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

    let reward = payment_purse_balance
        .checked_sub(refund)
        .ok_or(Error::ArithmeticOverflow)?;

    Ok((refund, reward))
}

/// Transfers funds from the payment purse to the validator rewards purse, as well as to the
/// refund purse, depending on how much was spent on the computation. This function maintains
/// the invariant that the balance of the payment purse is zero at the beginning and end of each
/// deploy and that the refund purse is unset at the beginning and end of each deploy.
pub(crate) fn finalize_payment<P: MintProvider + RuntimeProvider>(
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

    let (refund_amount, validator_reward) =
        calculate_refund_and_reward(gas_spent, payment_amount, provider.refund_handling())?;

    debug_assert_eq!(validator_reward + refund_amount, payment_amount);

    let refund_purse = get_refund_purse(provider)?;

    if let Some(refund_purse) = refund_purse {
        if refund_purse.remove_access_rights() == payment_purse.remove_access_rights() {
            // Make sure we're not refunding into a payment purse to invalidate payment code
            // postconditions.
            return Err(Error::RefundPurseIsPaymentPurse);
        }
    }

    provider.remove_key(REFUND_PURSE_KEY)?; //unset refund purse after reading it

    // Give or burn the refund.
    if !refund_amount.is_zero() {
        if target == URef::default() {
            // Fee-handling is set to `Burn`.  Deduct the refund from the payment purse and reduce
            // the total supply (i.e. burn the refund).
            payment_amount = payment_amount
                .checked_sub(refund_amount)
                .ok_or(Error::ArithmeticOverflow)?;

            provider.write_balance(payment_purse, payment_amount)?;
            provider.reduce_total_supply(refund_amount)?;
        } else {
            match refund_purse {
                Some(refund_purse) => {
                    // In case of failure to transfer to refund purse we fall back on the account's
                    // main purse
                    match provider.transfer_purse_to_purse(
                        payment_purse,
                        refund_purse,
                        refund_amount,
                    ) {
                        Ok(()) => {}
                        Err(error) => {
                            error!(
                                %error,
                                %refund_amount,
                                %account,
                                "unable to transfer refund to a refund purse; refunding to account"
                            );
                            refund_to_account::<P>(
                                provider,
                                payment_purse,
                                account,
                                refund_amount,
                            )?;
                        }
                    }
                }
                None => {
                    refund_to_account::<P>(provider, payment_purse, account, refund_amount)?;
                }
            }
        }
    }

    // Pay or burn the reward.
    if target == URef::default() {
        // Fee-handling is set to `Burn`.  Deduct the reward from the payment purse, leaving it
        // empty, and reduce the total supply (i.e. burn the reward).
        provider.write_balance(payment_purse, U512::zero())?;
        provider.reduce_total_supply(validator_reward)?;
    } else {
        // validator_reward purse is already resolved based on fee-handling config which is either a
        // proposer or rewards purse.
        match provider.transfer_purse_to_purse(payment_purse, target, validator_reward) {
            Ok(()) => {}
            Err(error) => {
                error!(%error, %validator_reward, %target, "unable to transfer reward");
                return Err(Error::FailedTransferToRewardsPurse);
            }
        }
    }
    Ok(())
}

pub(crate) fn refund_to_account<M: MintProvider>(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_move_dust_to_reward() {
        let purse_bal = U512::from(10u64);
        let gas = U512::from(3u64);
        let refund_ratio = Ratio::new_raw(1, 3);
        let refund = RefundHandling::Refund { refund_ratio };

        let (a, b) = calculate_refund_and_reward(gas, purse_bal, &refund).unwrap();

        assert_eq!(a, U512::from(2u64)); // (10 - 3) * 1/3 ~ 2.33 (.33 is dust)
        assert_eq!(b, U512::from(8u64)); // 10 - 2 = 8
    }

    #[test]
    fn should_account_refund_for_dust() {
        let purse_bal = U512::from(9973u64);
        let gas = U512::from(9161u64);

        for percentage in 0..=100 {
            let refund_ratio = Ratio::new_raw(percentage, 100);
            let refund = RefundHandling::Refund { refund_ratio };

            let (a, b) = calculate_refund_and_reward(gas, purse_bal, &refund).unwrap();

            let a = Ratio::from(a);
            let b = Ratio::from(b);

            assert_eq!(a + b, Ratio::from(purse_bal));
        }
    }
}
