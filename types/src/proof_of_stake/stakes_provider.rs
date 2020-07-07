use crate::proof_of_stake::{stakes::Stakes, Result};

/// A `StakesProvider` that reads and writes the stakes to/from the contract's known urefs.
pub trait StakesProvider {
    /// Read stakes map.
    fn read(&self) -> Result<Stakes>;

    /// Write stakes map.
    fn write(&mut self, stakes: &Stakes);
}
