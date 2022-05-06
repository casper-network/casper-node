use num_rational::Ratio;

/// Configuration options of fee elimination as part of handle payment finalization.

#[derive(Copy, Clone, Debug)]
pub enum FeeElimination {
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
