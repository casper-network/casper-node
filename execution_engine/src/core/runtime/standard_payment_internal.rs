use casper_types::{
    standard_payment::{AccountProvider, MintProvider, ProofOfStakeProvider, StandardPayment},
    system_contract_errors::pos,
    ApiError, Key, RuntimeArgs, URef, U512,
};

use crate::{
    core::{execution, runtime::Runtime},
    shared::stored_value::StoredValue,
    storage::global_state::StateReader,
};

pub(crate) const METHOD_GET_PAYMENT_PURSE: &str = "get_payment_purse";

/// Translates [`execution::Error`] into standard payment specific [`Error`].
///
/// This function is primarily used to propagate [`Error::GasLimit`] to make sure
/// [`StandardPayment`] contract running natively supports propagating gas limit errors without a
/// panic.
fn to_payment_error(exec_error: execution::Error, unhandled: ApiError) -> ApiError {
    match exec_error {
        execution::Error::GasLimit => pos::Error::GasLimit.into(),
        // There are possibly other exec errors happening but such transalation would be lossy.
        _ => unhandled,
    }
}

impl<'a, R> AccountProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_main_purse(&self) -> Result<URef, ApiError> {
        self.context
            .get_main_purse()
            .map_err(|e| to_payment_error(e, ApiError::InvalidPurse))
    }
}

impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ApiError> {
        let mint_contract_hash = self.get_mint_contract();
        match self.mint_transfer(mint_contract_hash, None, source, target, amount, None) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(api_error)) => Err(api_error),
            Err(exec_error) => Err(to_payment_error(exec_error, ApiError::Transfer)),
        }
    }
}

impl<'a, R> ProofOfStakeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_payment_purse(&mut self) -> Result<URef, ApiError> {
        let pos_contract_hash = self.get_pos_contract();

        let cl_value = self
            .call_contract(
                pos_contract_hash,
                METHOD_GET_PAYMENT_PURSE,
                RuntimeArgs::new(),
            )
            .map_err(|e| to_payment_error(e, pos::Error::PaymentPurseNotFound.into()))?;

        let payment_purse_ref: URef = cl_value.into_t()?;
        Ok(payment_purse_ref)
    }
}

impl<'a, R> StandardPayment for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
