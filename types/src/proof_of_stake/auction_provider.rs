use crate::{
    account::AccountHash, auction::SeigniorageRecipients, system_contract_errors::pos::Error, U512,
};
use alloc::vec::Vec;
use std::collections::BTreeSet;

pub trait AuctionProvider {
    fn read_winners(&mut self) -> BTreeSet<AccountHash> {
        todo!()
    }

    fn read_seigniorage_recipients(&mut self) -> SeigniorageRecipients {
        todo!()
    }

    /// Distributes `amount` to delegators associated with `validator_account_hash` proportional
    /// to delegated amount.
    fn distribute_to_delegators(
        &mut self,
        validator_account_hash: AccountHash,
        amount: U512,
    ) -> Result<(), Error> {
        todo!()
    }
}
