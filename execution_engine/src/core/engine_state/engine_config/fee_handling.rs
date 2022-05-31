/// Defines how fees are handled in the system.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FeeHandling {
    /// Transaction fees are paid to the block proposer.
    ///
    /// This is the default option for public chains.
    PayToProposer,
    /// Transaction fees are accumulated in a special purse and then distributed during end of era
    /// processing evenly among all administrator accounts.
    ///
    /// This setting is applicable for some private chains (but not all).
    Accumulate,
    /// Burn the fees.
    Burn,
}
