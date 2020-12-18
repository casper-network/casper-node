use casper_types::{
    account::AccountHash,
    proof_of_stake::{MintProvider, ProofOfStake, RuntimeProvider},
    system_contract_errors::pos::Error,
    ApiError, BlockTime, Key, Phase, TransferredTo, URef, U512,
};

use crate::{
    core::{execution, runtime::Runtime},
    shared::stored_value::StoredValue,
    storage::global_state::StateReader,
};

/// Translates [`execution::Error`] into PoS-specific [`Error`].
///
/// This function is primarily used to propagate [`Error::GasLimit`] to make sure [`ProofOfStake`]
/// contract running natively supports propagating gas limit errors without a panic.
fn to_pos_error(exec_error: execution::Error) -> Option<Error> {
    match exec_error {
        execution::Error::GasLimit => Some(Error::GasLimit),
        // There are possibly other exec errors happening but such transalation would be lossy.
        _ => None,
    }
}

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
        match self.transfer_from_purse_to_account(source, target, amount, None) {
            Ok(Ok(transferred_to)) => Ok(transferred_to),
            Ok(Err(api_error)) => Err(api_error),
            Err(exec_error) => Err(to_pos_error(exec_error).ok_or(ApiError::Transfer)?.into()),
        }
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ApiError> {
        let mint_contract_key = self.get_mint_contract();
        match self.mint_transfer(mint_contract_key, None, source, target, amount, None) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(api_error)) => Err(api_error),
            Err(exec_error) => Err(to_pos_error(exec_error).ok_or(ApiError::Transfer)?.into()),
        }
    }

    fn balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        match self.get_balance(purse) {
            Ok(maybe_balance) => Ok(maybe_balance),
            Err(exec_error) => Err(to_pos_error(exec_error).ok_or(Error::GetBalance)?),
        }
    }
}

// TODO: Update RuntimeProvider to better handle errors
impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_key(&self, name: &str) -> Result<Option<Key>, Error> {
        Ok(self.context.named_keys_get(name).cloned())
    }

    fn put_key(&mut self, name: &str, key: Key) -> Result<(), Error> {
        match self.context.put_key(name.to_string(), key) {
            Ok(()) => Ok(()),
            Err(exec_error) => Err(to_pos_error(exec_error).ok_or(Error::PutKey)?),
        }
    }

    fn remove_key(&mut self, name: &str) -> Result<(), Error> {
        match self.context.remove_key(name) {
            Ok(()) => Ok(()),
            Err(exec_error) => Err(to_pos_error(exec_error).ok_or(Error::RemoveKey)?),
        }
    }

    fn get_phase(&self) -> Result<Phase, Error> {
        Ok(self.context.phase())
    }

    fn get_block_time(&self) -> Result<BlockTime, Error> {
        Ok(self.context.get_blocktime())
    }

    fn get_caller(&self) -> Result<AccountHash, Error> {
        Ok(self.context.get_caller())
    }
}

impl<'a, R> ProofOfStake for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
