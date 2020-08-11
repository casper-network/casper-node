use crate::{
    account::AccountHash, auction::SeigniorageRecipients, system_contract_errors::pos::Error, URef,
};

use alloc::collections::BTreeSet;

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
        _validator_account_hash: AccountHash,
        _source_purse: URef,
    ) -> Result<(), Error> {
        // let args_values: RuntimeArgs = runtime_args! {
        //     ARG_VALIDATOR_ACCOUNT_HASH => validator_account_hash,
        //     ARG_SOURCE_PURSE => source_purse,
        // };

        // call_contract(system::get_auction(), METHOD_DISTRIBUTE_TO_DELEGATORS, args_values)

        // call_contract(system::get_auction(), ...)
        todo!()
    }
}
