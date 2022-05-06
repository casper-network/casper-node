use casper_types::{
    account::AccountHash,
    system::handle_payment::{Error, PAYMENT_PURSE_KEY, REFUND_PURSE_KEY},
    Key, Phase, PublicKey, URef, U512,
};
use num::{CheckedAdd, CheckedMul, CheckedSub, One};
use num_rational::Ratio;

use crate::core::engine_state::engine_config::FeeElimination;

use super::{mint_provider::MintProvider, runtime_provider::RuntimeProvider};

/// Returns the purse for accepting payment for transactions.
pub fn get_payment_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<URef, Error> {
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
pub fn get_refund_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<Option<URef>, Error> {
    match runtime_provider.get_key(REFUND_PURSE_KEY) {
        Some(Key::URef(uref)) => Ok(Some(uref)),
        Some(_) => Err(Error::RefundPurseKeyUnexpectedType),
        None => Ok(None),
    }
}

/// Returns tuple where 1st element is user part, and 2nd element is validator part.
fn calculate_refund_parts(
    amount_gas_spent: U512,
    payment_purse_balance: U512,
    refund_ratio: Ratio<u64>,
) -> Result<(U512, U512), Error> {
    let amount_gas_spent = Ratio::from(amount_gas_spent);
    let payment_purse_balance = Ratio::from(payment_purse_balance);

    let refund_amount = {
        let refund_amount_raw = payment_purse_balance
            .checked_sub(&amount_gas_spent)
            .ok_or(Error::ArithmeticOverflow)?;

        let refund_ratio_u512 = {
            let (numer, denom) = refund_ratio.into();
            debug_assert!(numer <= denom, "refund ratio should be a proper fraction");
            debug_assert!(denom > 0, "denominator should be greater than zero");
            Ratio::new_raw(U512::from_u64(numer), U512::from_u64(denom))
        };

        refund_amount_raw
            .checked_mul(&refund_ratio_u512)
            .ok_or(Error::ArithmeticOverflow)?
    };

    let validator_reward = payment_purse_balance
        .checked_sub(&refund_amount)
        .ok_or(Error::ArithmeticOverflow)?;

    let refund_amount_trunc = refund_amount.trunc();
    let validator_reward_trunc = validator_reward.trunc();

    let dust_amount = validator_reward
        .fract()
        .checked_add(&refund_amount.fract())
        .ok_or(Error::ArithmeticOverflow)?;

    // Give the dust amount to the user to reward him for depositing a larger than needed collateral
    // to execute the code.
    let user_part = refund_amount_trunc
        .checked_add(&dust_amount)
        .ok_or(Error::ArithmeticOverflow)?;
    debug_assert_eq!(user_part.fract(), Ratio::from(U512::zero()));

    let validator_part = validator_reward_trunc;
    debug_assert_eq!(validator_part.fract(), Ratio::from(U512::zero()));

    // Makes sure both parts: for user, and for validator sums to the total amount in the
    // payment's purse.
    debug_assert_eq!(
        user_part + validator_part,
        payment_purse_balance,
        "both user part and validator part should equal to the purse balance"
    );

    // Safely convert ratio to integer after making sure there's no dust amount left behind.
    Ok((user_part.to_integer(), validator_part.to_integer()))
}

/// Transfers funds from the payment purse to the validator rewards purse, as well as to the
/// refund purse, depending on how much was spent on the computation. This function maintains
/// the invariant that the balance of the payment purse is zero at the beginning and end of each
/// deploy and that the refund purse is unset at the beginning and end of each deploy.
pub fn finalize_payment<P: MintProvider + RuntimeProvider>(
    provider: &mut P,
    amount_spent: U512,
    account: AccountHash,
    target: URef,
) -> Result<(), Error> {
    let caller = provider.get_caller();
    if caller != PublicKey::System.to_account_hash() {
        return Err(Error::SystemFunctionCalledByUserAccount);
    }

    let payment_purse = get_payment_purse(provider)?;
    let total = match provider.balance(payment_purse)? {
        Some(balance) => balance,
        None => return Err(Error::PaymentPurseBalanceNotFound),
    };

    if total < amount_spent {
        return Err(Error::InsufficientPaymentForAmountSpent);
    }

    let refund_ratio = match provider.fee_elimination() {
        FeeElimination::Refund { refund_ratio } => *refund_ratio,
        FeeElimination::Accumulate => Ratio::one(), // Implied 100%
    };

    let (refund_amount, validator_reward) =
        calculate_refund_parts(amount_spent, total, refund_ratio)?;

    debug_assert_eq!(validator_reward + refund_amount, total);

    let refund_purse = get_refund_purse(provider)?;

    if let Some(refund_purse) = refund_purse {
        if refund_purse.remove_access_rights() == payment_purse.remove_access_rights() {
            // Make sure we're not refunding into a payment purse to invalidate payment code
            // postconditions.
            return Err(Error::RefundPurseIsPaymentPurse);
        }
    }

    provider.remove_key(REFUND_PURSE_KEY)?; //unset refund purse after reading it

    // validator_reward purse is already resolved based on fee elimination config which is either a
    // proposer or rewards purse
    provider
        .transfer_purse_to_purse(payment_purse, target, validator_reward)
        .map_err(|_| Error::FailedTransferToRewardsPurse)?;

    if refund_amount.is_zero() {
        return Ok(());
    }

    // give refund
    let refund_purse = match refund_purse {
        Some(uref) => uref,
        None => return refund_to_account::<P>(provider, payment_purse, account, refund_amount),
    };

    // in case of failure to transfer to refund purse we fall back on the account's main
    // purse
    if provider
        .transfer_purse_to_purse(payment_purse, refund_purse, refund_amount)
        .is_err()
    {
        return refund_to_account::<P>(provider, payment_purse, account, refund_amount);
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
            dbg!(error);
            Err(Error::FailedTransferToAccountPurse)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_account_refund_for_dust() {
        let purse_bal = U512::from(9973u64);
        let gas = U512::from(9161u64);

        for percentage in 0..=100 {
            let ratio = Ratio::new_raw(percentage, 100);

            let (a, b) = calculate_refund_parts(gas, purse_bal, ratio).unwrap();

            let a = Ratio::from(a);
            let b = Ratio::from(b);

            assert_eq!(a + b, Ratio::from(purse_bal));
        }
    }
}
