use super::{
    mint_provider::MintProvider, runtime_provider::RuntimeProvider,
    storage_provider::StorageProvider,
};
use casper_types::{
    system::handle_payment::{Error, PAYMENT_PURSE_KEY, REFUND_PURSE_KEY},
    Key, Phase, URef, U512,
};
use num::CheckedMul;
use num_rational::Ratio;
use num_traits::Zero;

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
pub fn calculate_overpayment_and_fee(
    limit: U512,
    gas_price: u8,
    cost: U512,
    consumed: U512,
    available_balance: U512,
    refund_ratio: Ratio<U512>,
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
    if refund_ratio.is_zero() {
        return Ok((U512::zero(), cost));
    }
    let unspent = limit.saturating_sub(consumed);
    if unspent == U512::zero() {
        return Ok((U512::zero(), cost));
    }
    let base_refund = unspent * gas_price;

    let adjusted_refund = Ratio::from(base_refund)
        .checked_mul(&refund_ratio)
        .ok_or(Error::ArithmeticOverflow)?
        .to_integer();

    let fee = cost
        .checked_sub(adjusted_refund)
        .ok_or(Error::ArithmeticOverflow)?;

    Ok((adjusted_refund, fee))
}

pub fn payment_burn<P: MintProvider + RuntimeProvider + StorageProvider>(
    provider: &mut P,
    purse: URef,
    amount: Option<U512>,
) -> Result<(), Error> {
    let available_balance = match provider.available_balance(purse)? {
        Some(balance) => balance,
        None => return Err(Error::PaymentPurseBalanceNotFound),
    };
    let burn_amount = amount.unwrap_or(available_balance);
    if burn_amount.is_zero() {
        // nothing to burn == noop
        return Ok(());
    }
    // Reduce the source purse and total supply by the refund amount
    let adjusted_balance = available_balance
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

    let administrative_accounts = provider.administrative_accounts();
    let reward_recipients = U512::from(administrative_accounts.len());

    let distribute_amount = match amount {
        Some(amount) => amount,
        None => provider.available_balance(source_uref)?.unwrap_or_default(),
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
    fn should_calculate_expected_amounts() {
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
            Ratio::new_raw(U512::from(1), U512::from(1)),
        )
        .unwrap();

        let unspent = limit.saturating_sub(consumed);
        let expected = unspent;
        assert_eq!(expected, overpay, "overpay");
        let expected_fee = consumed;
        assert_eq!(expected_fee, fee, "fee");
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
            let refund_ratio = Ratio::new_raw(U512::from(numer), U512::from(denom));
            let (overpay, fee) = calculate_overpayment_and_fee(
                limit,
                gas_price,
                cost,
                consumed,
                available,
                refund_ratio,
            )
            .unwrap();

            let unspent = limit.saturating_sub(consumed);
            let expected = Ratio::from(unspent)
                .checked_mul(&refund_ratio)
                .ok_or(Error::ArithmeticOverflow)
                .expect("should math")
                .to_integer();
            assert_eq!(expected, overpay, "overpay");
            let expected_fee = limit - expected;
            assert_eq!(expected_fee, fee, "fee");
        }
    }

    #[test]
    fn should_roll_over_dust() {
        let limit = U512::from(6u64);
        let gas_price = 1;
        let cost = limit;
        let consumed = U512::from(3u64);
        let available = U512::from(10u64);

        for percentage in 0..=100 {
            let refund_ratio = Ratio::new_raw(U512::from(percentage), U512::from(100));

            let (overpay, fee) = calculate_overpayment_and_fee(
                limit,
                gas_price,
                cost,
                consumed,
                available,
                refund_ratio,
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
            Ratio::new_raw(U512::from(1), U512::from(1)),
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
            Ratio::new_raw(U512::from(1), U512::from(1)),
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
            Ratio::new_raw(U512::from(1), U512::from(1)),
        )
        .unwrap();

        let unspent = limit.saturating_sub(consumed);
        let expected = unspent * gas_price;
        assert_eq!(expected, overpay, "overpay");
        let expected_fee = consumed * gas_price + extra_cost;
        assert_eq!(expected_fee, fee, "fee");
    }
}
