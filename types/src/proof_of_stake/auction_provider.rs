use crate::{
    account::AccountHash, auction::{METHOD_DISTRIBUTE_TO_DELEGATORS, SeigniorageRecipients, ARG_VALIDATOR_ACCOUNT_HASH, ARG_SOURCE_PURSE}, system_contract_errors::pos::Error, URef, RuntimeArgs, runtime_args,
};

use alloc::collections::BTreeSet;

pub trait AuctionProvider {
    fn read_winners(&mut self) -> BTreeSet<AccountHash>;

    fn read_seigniorage_recipients(&mut self) -> SeigniorageRecipients;

    /// Distributes `amount` to delegators associated with `validator_account_hash` proportional
    /// to delegated amount.
    fn distribute_to_delegators(
        &mut self,
        validator_account_hash: AccountHash,
        source_purse: URef,
    ) -> Result<(), Error>;
}
