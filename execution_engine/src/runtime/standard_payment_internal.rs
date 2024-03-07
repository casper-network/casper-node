use casper_storage::global_state::{error::Error as GlobalStateError, state::StateReader};
use casper_types::{
    account::Account,
    system::{handle_payment, mint},
    ApiError, Key, RuntimeArgs, StoredValue, TransferredTo, URef, U512,
};

use casper_storage::system::standard_payment::{
    account_provider::AccountProvider, handle_payment_provider::HandlePaymentProvider,
    mint_provider::MintProvider, StandardPayment,
};

use crate::{execution, runtime::Runtime};

pub(crate) const METHOD_GET_PAYMENT_PURSE: &str = "get_payment_purse";

impl From<execution::Error> for Option<ApiError> {
    fn from(exec_error: execution::Error) -> Self {
        match exec_error {
            // This is used to propagate [`execution::Error::GasLimit`] to make sure
            // [`StandardPayment`] contract running natively supports propagating gas limit
            // errors without a panic.
            execution::Error::GasLimit => Some(mint::Error::GasLimit.into()),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
    }
}

impl<'a, R> AccountProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_main_purse(&mut self) -> Result<URef, ApiError> {
        self.context.get_main_purse().map_err(|exec_error| {
            <Option<ApiError>>::from(exec_error).unwrap_or(ApiError::InvalidPurse)
        })
    }
}

impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target_account: &Account,
        amount: U512,
    ) -> Result<(), ApiError> {
        match Runtime::transfer_from_purse_to_account(self, source, target_account, amount, None) {
            Ok(Ok(TransferredTo::ExistingAccount)) => Ok(()),
            Ok(Ok(TransferredTo::NewAccount)) => Ok(()),
            Ok(Err(error)) => Err(error),
            Err(_error) => Err(ApiError::Transfer),
        }
    }
}

impl<'a, R> HandlePaymentProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_payment_purse(&mut self) -> Result<URef, ApiError> {
        let handle_payment_contract_hash = self
            .get_handle_payment_contract()
            .map_err(|_| ApiError::MissingSystemContractHash)?;

        let cl_value = self
            .call_contract(
                handle_payment_contract_hash,
                METHOD_GET_PAYMENT_PURSE,
                RuntimeArgs::new(),
            )
            .map_err(|exec_error| {
                let maybe_api_error: Option<ApiError> = exec_error.into();
                maybe_api_error
                    .unwrap_or_else(|| handle_payment::Error::PaymentPurseNotFound.into())
            })?;

        let payment_purse_ref: URef = cl_value.into_t()?;
        Ok(payment_purse_ref)
    }
}

impl<'a, R> StandardPayment for Runtime<'a, R> where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>
{
}
