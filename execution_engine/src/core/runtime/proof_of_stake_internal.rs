use casper_types::{
    account::AccountHash,
    proof_of_stake::{MintProvider, ProofOfStake, RuntimeProvider},
    ApiError, BlockTime, Key, Phase, TransferredTo, URef, U512,
};

use crate::{
    core::{execution, runtime::Runtime},
    shared::stored_value::StoredValue,
    storage::global_state::StateReader,
};

// TODO: Update MintProvider to better handle errors
impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> Result<TransferredTo, ApiError> {
        self.transfer_from_purse_to_account(source, target, amount)
            .expect("should transfer from purse to account")
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ()> {
        let mint_contract_key = self.get_mint_contract();
        if self
            .mint_transfer(mint_contract_key, source, target, amount)
            .is_ok()
        {
            Ok(())
        } else {
            Err(())
        }
    }

    fn balance(&mut self, purse: URef) -> Option<U512> {
        self.get_balance(purse).expect("should get balance")
    }
}

// TODO: Update RuntimeProvider to better handle errors
impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_key(&self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn put_key(&mut self, name: &str, key: Key) {
        self.context
            .put_key(name.to_string(), key)
            .expect("should put key")
    }

    fn remove_key(&mut self, name: &str) {
        self.context.remove_key(name).expect("should remove key")
    }

    fn get_phase(&self) -> Phase {
        self.context.phase()
    }

    fn get_block_time(&self) -> BlockTime {
        self.context.get_blocktime()
    }

    fn get_caller(&self) -> AccountHash {
        self.context.get_caller()
    }
}

impl<'a, R> ProofOfStake for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
