/// Configuration options of refund handling that is executed as part of handle payment
/// finalization.
use num_rational::Ratio;

/// Defines how refunds are calculated.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RefundHandling {
    /// Refund of excess payment amount goes to either a pre-defined purse, or back to the sender
    /// and the rest of the payment amount goes to the block proposer.
    Refund {
        /// Computes how much refund goes back to the user after deducting gas spent from the paid
        /// amount.
        ///
        /// user_part = (payment_amount - gas_spent_amount) * refund_ratio
        /// validator_part = payment_amount - user_part
        ///
        /// Any dust amount that was a result of multiplying by refund_ratio goes back to user.
        refund_ratio: Ratio<u64>,
    },
}
