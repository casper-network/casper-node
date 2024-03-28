use casper_types::{
    system::handle_payment::{Error, PAYMENT_PURSE_KEY, REFUND_PURSE_KEY},
    FeeHandling, HoldsEpoch, Key, Phase, PublicKey, RefundHandling, URef, U512,
};
use num::{CheckedMul, One};
use num_rational::Ratio;
use num_traits::Zero;
use tracing::error;

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

/// Returns tuple where 1st element is the portion of unspent payment (if any), and the 2nd element
/// is the fee (if any).
///
/// # Note
///
/// Any dust amounts are added to the fee.
fn calculate_overpayment_and_fee(
    limit: U512,
    gas_price: u8,
    cost: U512,
    consumed: U512,
    available_balance: U512,
    refund_handling: RefundHandling,
) -> Result<(U512, U512), Error> {
    /*
        cost is limit * price,  unused = limit - consumed
        base refund is unused * price
            refund rate is a percentage ranging from 0% to 100%
            actual refund = base refund * refund rate
            i.e. if rate == 100%, actual refund == base refund
                 if rate = 0%, actual refund = 0 (and we can skip refund processing)
        EXAMPLE 1
        limit = 500, consumed = 450, price = 2, refund rate = 100%
        cost = limit * price == 1000
        unused = limit - consumed == 50
        base refund = unused * price == 100
        actual refund = base refund * refund rate == 100

        EXAMPLE 2
        limit = 5000, consumed = 0, price = 5, refund rate = 50%
        cost = limit * price == 25000
        unused = limit - consumed == 5000
        base refund = unused * price == 25000
        actual refund = base refund * refund rate == 12500

        Complicating factors:
            if the source purse does not have enough to cover the cost, their available balance is taken
                and there is no refund
            if the refund rate is 0%, there is no refund (although it would be bizarre for a network to
                run with RefundHandling turned on but with a 0% rate, they are technically independent
                settings and thus the logic must account for the possibility)
            cost might be higher than limit * price if additional costs have been incurred.
                as the refund calculation is based on paid for but unused gas, such additional costs
                are not subject to refund. This is handled by this logic correctly, but tests over logic
                that incurs any additional costs need to use actual discrete variables for each value
                and not assume limit * price == cost
    */
    if available_balance < cost {
        return Ok((U512::zero(), available_balance));
    }
    if refund_handling.skip_refund() {
        return Ok((U512::zero(), cost));
    }
    let unspent = limit.saturating_sub(consumed);
    if unspent == U512::zero() {
        return Ok((U512::zero(), cost));
    }
    let base_refund = unspent * gas_price;
    let refund_ratio = match refund_handling {
        RefundHandling::Refund { refund_ratio } | RefundHandling::Burn { refund_ratio } => {
            debug_assert!(
                refund_ratio <= Ratio::one(),
                "refund ratio should be a proper fraction"
            );
            let (numer, denom) = refund_ratio.into();
            Ratio::new_raw(U512::from(numer), U512::from(denom))
        }
        RefundHandling::NoRefund => Ratio::zero(),
    };

    let adjusted_refund = Ratio::from(base_refund)
        .checked_mul(&refund_ratio)
        .ok_or(Error::ArithmeticOverflow)?
        .to_integer();

    let fee = cost
        .checked_sub(adjusted_refund)
        .ok_or(Error::ArithmeticOverflow)?;

    Ok((adjusted_refund, fee))
}

/// This function handles payment post-processing to pay out fees and refunds.
///
/// The behavior of this function is very load bearing and complex, based on every possible
/// combination of [`FeeHandling`] and [`RefundHandling`] handling variants.
///
/// NOTE: If a network is configured for both NoFee and NoRefund, this method will error if called.
#[allow(clippy::too_many_arguments)]
pub fn finalize_payment<P: MintProvider + RuntimeProvider + StorageProvider>(
    provider: &mut P,
    limit: U512,
    gas_price: u8,
    cost: U512,
    consumed: U512,
    source_purse: URef,
    target_purse: URef,
    holds_epoch: HoldsEpoch,
) -> Result<(), Error> {
    let refund_handling = provider.refund_handling();
    let fee_handling = provider.fee_handling();
    if fee_handling.skip_fee_handling() && refund_handling.skip_refund() {
        // this method should not even be called if NoFee && NoRefund are set,
        //   as there is nothing to finalize
        return Err(Error::IncompatiblePaymentSettings);
    }

    let caller = provider.get_caller();
    if caller != PublicKey::System.to_account_hash() {
        return Err(Error::SystemFunctionCalledByUserAccount);
    }

    let source_available_balance = match provider.available_balance(source_purse, holds_epoch)? {
        Some(balance) => balance,
        None => return Err(Error::PaymentPurseBalanceNotFound),
    };

    let (refund, fee) = calculate_overpayment_and_fee(
        limit,
        gas_price,
        cost,
        consumed,
        source_available_balance,
        refund_handling,
    )?;

    if !refund.is_zero() {
        match refund_handling {
            RefundHandling::Refund { .. } => {
                let maybe_refund_purse = get_refund_purse(provider)?;
                if let Some(refund_purse) = maybe_refund_purse {
                    // refund purse cannot also be source purse
                    if refund_purse.remove_access_rights() == source_purse.remove_access_rights() {
                        return Err(Error::RefundPurseIsPaymentPurse);
                    }
                    //unset refund purse after reading it
                    provider.remove_key(REFUND_PURSE_KEY)?;
                    if let Err(error) =
                        provider.transfer_purse_to_purse(source_purse, refund_purse, refund)
                    {
                        error!(
                            %error,
                            %refund,
                            %source_purse,
                            %refund_purse,
                            "unable to transfer refund to a refund purse"
                        );
                    }
                }
            }
            RefundHandling::Burn { .. } => {
                burn(provider, source_purse, Some(refund))?;
            }
            RefundHandling::NoRefund => {
                // this must be due to either programmer error or invalid chainspec settings
                return Err(Error::IncompatiblePaymentSettings);
            }
        }
    }

    // Pay or burn the fee.
    match fee_handling {
        FeeHandling::PayToProposer | FeeHandling::Accumulate => {
            match provider.transfer_purse_to_purse(source_purse, target_purse, fee) {
                Ok(()) => {}
                Err(error) => {
                    error!(%error, %fee, %target_purse, "unable to transfer fee");
                    return Err(Error::FailedTransferToRewardsPurse);
                }
            }
        }
        FeeHandling::Burn => {
            burn(provider, source_purse, Some(refund))?;
        }
        FeeHandling::NoFee => {
            if !fee.is_zero() {
                // this must be due to either programmer error or invalid chainspec settings
                return Err(Error::IncompatiblePaymentSettings);
            }
        }
    }
    Ok(())
}

pub fn burn<P: MintProvider + RuntimeProvider + StorageProvider>(
    provider: &mut P,
    purse: URef,
    amount: Option<U512>,
) -> Result<(), Error> {
    // get the purse total balance (without holds)
    let total_balance = match provider.available_balance(purse, HoldsEpoch::NOT_APPLICABLE)? {
        Some(balance) => balance,
        None => return Err(Error::PaymentPurseBalanceNotFound),
    };
    let burn_amount = amount.unwrap_or(total_balance);
    if burn_amount.is_zero() {
        // nothing to burn == noop
        return Ok(());
    }
    // Reduce the source purse and total supply by the refund amount
    let adjusted_balance = total_balance
        .checked_sub(burn_amount)
        .ok_or(Error::ArithmeticOverflow)?;
    provider.write_balance(purse, adjusted_balance)?;
    provider.reduce_total_supply(burn_amount)?;
    Ok(())
}

/// This function distributes the fees according to the fee handling config.
///
/// NOTE: If a network is not configured for fee accumulation, this method will error if called.
pub fn distribute_accumulated_fees<P>(
    provider: &mut P,
    source_uref: URef,
    amount: Option<U512>,
) -> Result<(), Error>
where
    P: RuntimeProvider + MintProvider,
{
    let fee_handling = provider.fee_handling();
    if !fee_handling.is_accumulate() {
        return Err(Error::IncompatiblePaymentSettings);
    }

    if provider.get_caller() != PublicKey::System.to_account_hash() {
        return Err(Error::SystemFunctionCalledByUserAccount);
    }

    let administrative_accounts = provider.administrative_accounts();
    let reward_recipients = U512::from(administrative_accounts.len());

    let distribute_amount = match amount {
        Some(amount) => amount,
        None => provider
            .available_balance(source_uref, HoldsEpoch::NOT_APPLICABLE)?
            .unwrap_or_default(),
    };

    if distribute_amount.is_zero() {
        return Ok(());
    }

    let portion = distribute_amount
        .checked_div(reward_recipients)
        .unwrap_or_else(U512::zero);

    if !portion.is_zero() {
        for target in administrative_accounts {
            provider.transfer_purse_to_account(source_uref, target, portion)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // both burn and refund use the same basic calculation for
    // overpayment / unspent vs fee...the only difference is
    // what is done with the overage _after_ the calculation
    // refund returns it to payer, while burn destroys it

    #[test]
    fn should_burn_expected_amount() {
        let handling = RefundHandling::Burn {
            refund_ratio: Ratio::new_raw(1, 1),
        };
        test_handle_payment(handling);
    }

    #[test]
    fn should_refund_expected_amount() {
        let handling = RefundHandling::Refund {
            refund_ratio: Ratio::new_raw(1, 1),
        };
        test_handle_payment(handling);
    }

    #[test]
    fn should_handle_straight_percentages() {
        let limit = U512::from(100u64);
        let gas_price = 1;
        let cost = limit;
        let consumed = U512::from(50u64);
        let available = U512::from(1000u64);
        let denom = 100;

        for numer in 0..=denom {
            let refund_ratio = Ratio::new_raw(numer, denom);
            let handling = RefundHandling::Refund { refund_ratio };
            let (overpay, fee) = calculate_overpayment_and_fee(
                limit, gas_price, cost, consumed, available, handling,
            )
            .unwrap();

            let unspent = limit.saturating_sub(consumed).as_u64();
            let expected = Ratio::from(unspent)
                .checked_mul(&refund_ratio)
                .ok_or(Error::ArithmeticOverflow)
                .expect("should math")
                .to_integer();
            assert_eq!(expected, overpay.as_u64(), "overpay");
            let expected_fee = limit.as_u64() - expected;
            assert_eq!(expected_fee, fee.as_u64(), "fee");
        }
    }

    fn test_handle_payment(refund_handling: RefundHandling) {
        let limit = U512::from(6u64);
        let gas_price = 1;
        let cost = limit;
        let consumed = U512::from(3u64);
        let available = U512::from(10u64);
        let (overpay, fee) = calculate_overpayment_and_fee(
            limit,
            gas_price,
            cost,
            consumed,
            available,
            refund_handling,
        )
        .unwrap();

        let unspent = limit.saturating_sub(consumed);
        let expected = unspent;
        assert_eq!(expected, overpay, "{:?}", refund_handling);
        let expected_fee = consumed;
        assert_eq!(expected_fee, fee, "fee");
    }

    #[test]
    fn should_roll_over_dust() {
        let limit = U512::from(6u64);
        let gas_price = 1;
        let cost = limit;
        let consumed = U512::from(3u64);
        let available = U512::from(10u64);

        for percentage in 0..=100 {
            let handling = RefundHandling::Refund {
                refund_ratio: Ratio::new_raw(percentage, 100),
            };

            let (overpay, fee) = calculate_overpayment_and_fee(
                limit, gas_price, cost, consumed, available, handling,
            )
            .expect("should have overpay and fee");

            let a = Ratio::from(overpay);
            let b = Ratio::from(fee);

            assert_eq!(a + b, Ratio::from(cost), "{}", percentage);
        }
    }

    #[test]
    fn should_take_all_of_insufficient_balance() {
        let limit = U512::from(6u64);
        let gas_price = 1;
        let cost = limit;
        let consumed = U512::from(3u64);
        let available = U512::from(5u64);

        let (overpay, fee) = calculate_overpayment_and_fee(
            limit,
            gas_price,
            cost,
            consumed,
            available,
            RefundHandling::Refund {
                refund_ratio: Ratio::new_raw(1, 1),
            },
        )
        .unwrap();

        assert_eq!(U512::zero(), overpay, "overpay");
        let expected = available;
        assert_eq!(expected, fee, "fee");
    }

    #[test]
    fn should_handle_non_1_gas_price() {
        let limit = U512::from(6u64);
        let gas_price = 2;
        let cost = limit * gas_price;
        let consumed = U512::from(3u64);
        let available = U512::from(12u64);

        let (overpay, fee) = calculate_overpayment_and_fee(
            limit,
            gas_price,
            cost,
            consumed,
            available,
            RefundHandling::Refund {
                refund_ratio: Ratio::new_raw(1, 1),
            },
        )
        .unwrap();

        let unspent = limit.saturating_sub(consumed);
        let expected = unspent * gas_price;
        assert_eq!(expected, overpay, "overpay");
        let expected_fee = consumed * gas_price;
        assert_eq!(expected_fee, fee, "fee");
    }

    #[test]
    fn should_handle_extra_cost() {
        let limit = U512::from(6u64);
        let gas_price = 2;
        let extra_cost = U512::from(1u64);
        let cost = limit * gas_price + extra_cost;
        let consumed = U512::from(3u64);
        let available = U512::from(21u64);

        let (overpay, fee) = calculate_overpayment_and_fee(
            limit,
            gas_price,
            cost,
            consumed,
            available,
            RefundHandling::Refund {
                refund_ratio: Ratio::new_raw(1, 1),
            },
        )
        .unwrap();

        let unspent = limit.saturating_sub(consumed);
        let expected = unspent * gas_price;
        assert_eq!(expected, overpay, "overpay");
        let expected_fee = consumed * gas_price + extra_cost;
        assert_eq!(expected_fee, fee, "fee");
    }
}
