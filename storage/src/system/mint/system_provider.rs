use casper_types::{account::AccountHash, system::mint::Error, URef, U512};

/// Provides functionality of a system module.
pub trait SystemProvider {
    /* TODO: record_transfer:: writing Deploy / TransactionInfo and Transfer records to global state
       is not sustainable as such records are never pruned away, causing the width of a
       slice of global state at tip to unnecessarily grow as they accrete over time.
       Instead, this salient details can be recored in the transaction meta data that we
       already store ... currently a vec of TransferAddr is stored. Going forward we would
       just store the record itself in the metadata. That metadata is already wired up
       to be synchronized, chunked, etc.
       In the meantime, until that can be wired up, disabling the storing of these records.
    */

    /// Records a transfer.
    fn record_transfer(
        &mut self,
        maybe_to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), Error>;
}
