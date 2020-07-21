use std::{cell::RefCell, rc::Rc};

use types::{account::AccountHash, AccessRights, ApiError, Key, RuntimeArgs, URef, U512};

use crate::components::contract_runtime::{
    core::{
        engine_state::Error,
        execution::Error as ExecError,
        tracking_copy::{TrackingCopy, TrackingCopyExt},
    },
    shared,
    shared::{account::Account, newtypes::CorrelationId, stored_value::StoredValue},
    storage::global_state::StateReader,
};

const SOURCE: &str = "source";
const TARGET: &str = "target";
const AMOUNT: &str = "amount";

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TransferTargetMode {
    Unknown,
    PurseExists(URef),
    CreateAccount(AccountHash),
}

pub struct TransferRuntimeArgsBuilder {
    inner: RuntimeArgs,
    transfer_target_mode: TransferTargetMode,
}

impl TransferRuntimeArgsBuilder {
    pub fn new(imputed_runtime_args: RuntimeArgs) -> TransferRuntimeArgsBuilder {
        TransferRuntimeArgsBuilder {
            inner: imputed_runtime_args,
            transfer_target_mode: TransferTargetMode::Unknown,
        }
    }

    fn purse_exists<R>(
        &self,
        uref: URef,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> bool
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        // it is a URef but is it a purse URef?
        tracking_copy
            .borrow_mut()
            .get_purse_balance_key(correlation_id, uref.into())
            .is_ok()
    }

    fn resolve_source_uref<R>(
        &self,
        account: &Account,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<URef, Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        let imputed_runtime_args = &self.inner;
        let arg_name = SOURCE;
        match imputed_runtime_args.get(arg_name) {
            Some(cl_value) if *cl_value.cl_type() == types::CLType::URef => {
                let uref: URef = match cl_value.clone().into_t() {
                    Ok(uref) => uref,
                    Err(error) => {
                        return Err(Error::Exec(ExecError::Revert(error.into())));
                    }
                };
                if account.main_purse().addr() == uref.addr() {
                    return Ok(uref);
                }

                let normalized_uref = Key::URef(uref).normalize();
                let maybe_named_key = account
                    .named_keys()
                    .values()
                    .find(|&named_key| named_key.normalize() == normalized_uref);
                match maybe_named_key {
                    Some(Key::URef(found_uref)) => {
                        if found_uref.is_writeable() {
                            // it is a URef and caller has access but is it a purse URef?
                            if !self.purse_exists(
                                found_uref.to_owned(),
                                correlation_id,
                                tracking_copy,
                            ) {
                                return Err(Error::Exec(ExecError::Revert(ApiError::InvalidPurse)));
                            }

                            Ok(uref)
                        } else {
                            Err(Error::Exec(ExecError::InvalidAccess {
                                required: AccessRights::WRITE,
                            }))
                        }
                    }
                    Some(key) => Err(Error::Exec(ExecError::TypeMismatch(
                        shared::TypeMismatch::new("Key::URef".to_string(), key.type_string()),
                    ))),
                    None => Err(Error::Exec(ExecError::ForgedReference(uref))),
                }
            }
            Some(_) => Err(Error::Exec(ExecError::Revert(ApiError::InvalidArgument))),
            None => Ok(account.main_purse()), // if no source purse passed use account main purse
        }
    }

    fn resolve_transfer_target_mode<R>(
        &self,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<TransferTargetMode, Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        let imputed_runtime_args = &self.inner;
        let arg_name = TARGET;
        match imputed_runtime_args.get(arg_name) {
            Some(cl_value) if *cl_value.cl_type() == types::CLType::URef => {
                let uref: URef = match cl_value.clone().into_t() {
                    Ok(uref) => uref,
                    Err(error) => {
                        return Err(Error::Exec(ExecError::Revert(error.into())));
                    }
                };

                if !self.purse_exists(uref, correlation_id, tracking_copy) {
                    return Err(Error::Exec(ExecError::Revert(ApiError::InvalidPurse)));
                }

                Ok(TransferTargetMode::PurseExists(uref))
            }
            Some(cl_value)
                if *cl_value.cl_type()
                    == types::CLType::FixedList(Box::new(types::CLType::U8), 32) =>
            {
                let account_key: Key = {
                    let hash = match cl_value.clone().into_t() {
                        Ok(hash) => hash,
                        Err(error) => {
                            return Err(Error::Exec(ExecError::Revert(error.into())));
                        }
                    };
                    Key::Account(hash)
                };
                match account_key.into_account() {
                    Some(public_key) => {
                        match tracking_copy
                            .borrow_mut()
                            .read_account(correlation_id, public_key)
                        {
                            Ok(account) => {
                                Ok(TransferTargetMode::PurseExists(account.main_purse()))
                            }
                            Err(_) => Ok(TransferTargetMode::CreateAccount(public_key)),
                        }
                    }
                    None => Err(Error::Exec(ExecError::Revert(ApiError::Transfer))),
                }
            }
            Some(cl_value) if *cl_value.cl_type() == types::CLType::Key => {
                let account_key: Key = match cl_value.clone().into_t() {
                    Ok(key) => key,
                    Err(error) => {
                        return Err(Error::Exec(ExecError::Revert(error.into())));
                    }
                };
                match account_key.into_account() {
                    Some(public_key) => {
                        match tracking_copy
                            .borrow_mut()
                            .read_account(correlation_id, public_key)
                        {
                            Ok(account) => {
                                Ok(TransferTargetMode::PurseExists(account.main_purse()))
                            }
                            Err(_) => Ok(TransferTargetMode::CreateAccount(public_key)),
                        }
                    }
                    None => Err(Error::Exec(ExecError::Revert(ApiError::Transfer))),
                }
            }
            Some(_) => Err(Error::Exec(ExecError::Revert(ApiError::InvalidArgument))),
            None => Err(Error::Exec(ExecError::Revert(ApiError::MissingArgument))),
        }
    }

    fn resolve_amount(&self) -> Result<U512, Error> {
        let imputed_runtime_args = &self.inner;
        match imputed_runtime_args.get(AMOUNT) {
            Some(amount_value) if *amount_value.cl_type() == types::CLType::U512 => {
                match amount_value.clone().into_t::<U512>() {
                    Ok(amount) => {
                        if amount == U512::zero() {
                            Err(Error::Exec(ExecError::Revert(ApiError::Transfer)))
                        } else {
                            Ok(amount)
                        }
                    }
                    Err(error) => Err(Error::Exec(ExecError::Revert(error.into()))),
                }
            }
            Some(amount_value) if *amount_value.cl_type() == types::CLType::U64 => {
                match amount_value.clone().into_t::<u64>() {
                    Ok(amount) => match amount {
                        0 => Err(Error::Exec(ExecError::Revert(ApiError::Transfer))),
                        _ => Ok(U512::from(amount)),
                    },
                    Err(error) => Err(Error::Exec(ExecError::Revert(error.into()))),
                }
            }
            Some(_) => Err(Error::Exec(ExecError::Revert(ApiError::InvalidArgument))),
            None => Err(Error::Exec(ExecError::Revert(ApiError::MissingArgument))),
        }
    }

    pub fn transfer_target_mode<R>(
        &mut self,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<TransferTargetMode, Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        let mode = self.transfer_target_mode;
        if mode != TransferTargetMode::Unknown {
            return Ok(mode);
        }
        match self.resolve_transfer_target_mode(correlation_id, tracking_copy) {
            Ok(mode) => {
                self.transfer_target_mode = mode;
                Ok(mode)
            }
            Err(error) => Err(error),
        }
    }

    pub fn build<R>(
        self,
        account: &Account,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<RuntimeArgs, Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        let target_uref =
            match self.resolve_transfer_target_mode(correlation_id, Rc::clone(&tracking_copy))? {
                TransferTargetMode::PurseExists(uref) => uref,
                _ => {
                    return Err(Error::Exec(ExecError::Revert(ApiError::Transfer)));
                }
            };

        let source_uref =
            self.resolve_source_uref(account, correlation_id, Rc::clone(&tracking_copy))?;

        if source_uref.addr() == target_uref.addr() {
            return Err(ExecError::Revert(ApiError::InvalidPurse).into());
        }

        let amount = self.resolve_amount()?;

        let runtime_args = {
            let mut runtime_args = RuntimeArgs::new();

            runtime_args.insert(SOURCE, source_uref);
            runtime_args.insert(TARGET, target_uref);
            runtime_args.insert(AMOUNT, amount);

            runtime_args
        };

        Ok(runtime_args)
    }
}
