use crate::{system_contract_errors::mint::Error, URef, U512};

/// Provides functionality of a system module.
pub trait SystemProvider {
    /// Records a transfer.
    fn record_transfer(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), Error>;
}
