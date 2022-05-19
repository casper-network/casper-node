/// Defines how fees are handled in the system.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FeeHandling {
    /// Transaction fees are paid to the block proposer.
    ///
    /// This is the default option for public chains.
    PayToProposer,
    /// Transaction fees are accumulated in a special rewards purse.
    ///
    /// This setting makes sense for some private chains.
    Accumulate,
}
