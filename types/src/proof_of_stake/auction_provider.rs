use crate::{
    account::AccountHash, auction::{METHOD_DISTRIBUTE_TO_DELEGATORS, SeigniorageRecipients, ARG_SOURCE_PURSE}, system_contract_errors::pos::Error, URef, RuntimeArgs, runtime_args, PublicKey,
};

use alloc::collections::BTreeSet;

pub trait AuctionProvider {
    fn read_winners(&mut self) -> BTreeSet<AccountHash>;

    fn read_seigniorage_recipients(&mut self) -> SeigniorageRecipients;

    fn distribute_to_delegators(
        &mut self,
        validator_public_key: PublicKey,
        purse: URef,
    ) -> Result<(), Error>;
}
