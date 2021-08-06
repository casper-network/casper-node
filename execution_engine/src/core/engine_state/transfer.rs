use std::{cell::RefCell, convert::TryFrom, rc::Rc};

use casper_types::{
    account::AccountHash, system::mint, AccessRights, ApiError, CLType, CLValueError, Key,
    RuntimeArgs, URef, U512,
};

use crate::{
    core::{
        engine_state::Error,
        execution::Error as ExecError,
        tracking_copy::{TrackingCopy, TrackingCopyExt},
    },
    shared::{self, account::Account, newtypes::CorrelationId, stored_value::StoredValue},
    storage::global_state::StateReader,
};

/// A target mode indicates if a native transfer's arguments will resolve to an existing purse, or
/// will have to create a new account first.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TransferTargetMode {
    /// Unknown target mode.
    Unknown,
    /// Native transfer arguments resolved into a transfer to a purse.
    PurseExists(URef),
    /// Native transfer arguments resolved into a transfer to an account.
    CreateAccount(AccountHash),
}

/// Mint's transfer arguments.
///
/// A struct has a benefit of static typing, which is helpful while resolving the arguments.
#[derive(Debug, Clone, Copy)]
pub struct TransferArgs {
    to: Option<AccountHash>,
    source: URef,
    target: URef,
    amount: U512,
    arg_id: Option<u64>,
}

impl TransferArgs {
    /// Creates new transfer arguments.
    pub fn new(
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        arg_id: Option<u64>,
    ) -> Self {
        Self {
            to,
            source,
            target,
            amount,
            arg_id,
        }
    }

    /// Returns `to` field.
    pub fn to(&self) -> Option<AccountHash> {
        self.to
    }

    /// Returns `source` field.
    pub fn source(&self) -> URef {
        self.source
    }

    /// Returns `arg_id` field.
    pub fn arg_id(&self) -> Option<u64> {
        self.arg_id
    }

    /// Returns `amount` field.
    pub fn amount(&self) -> U512 {
        self.amount
    }
}

impl TryFrom<TransferArgs> for RuntimeArgs {
    type Error = CLValueError;

    fn try_from(transfer_args: TransferArgs) -> Result<Self, Self::Error> {
        let mut runtime_args = RuntimeArgs::new();

        runtime_args.insert(mint::ARG_TO, transfer_args.to)?;
        runtime_args.insert(mint::ARG_SOURCE, transfer_args.source)?;
        runtime_args.insert(mint::ARG_TARGET, transfer_args.target)?;
        runtime_args.insert(mint::ARG_AMOUNT, transfer_args.amount)?;
        runtime_args.insert(mint::ARG_ID, transfer_args.arg_id)?;

        Ok(runtime_args)
    }
}

/// State of a transfer args builder.
///
/// Purpose of this builder is to resolve native tranfer args into [`TransferTargetMode`] and a
/// [`TransferArgs`] instance to execute actual token transfer on the mint contract.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferRuntimeArgsBuilder {
    inner: RuntimeArgs,
    transfer_target_mode: TransferTargetMode,
    to: Option<AccountHash>,
}

impl TransferRuntimeArgsBuilder {
    /// Creates new transfer args builder.
    ///
    /// Takes an incoming runtime args that represents native transfer's arguments.
    pub fn new(imputed_runtime_args: RuntimeArgs) -> TransferRuntimeArgsBuilder {
        TransferRuntimeArgsBuilder {
            inner: imputed_runtime_args,
            transfer_target_mode: TransferTargetMode::Unknown,
            to: None,
        }
    }

    /// Checks if a purse exists.
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
        let key = match tracking_copy
            .borrow_mut()
            .get_purse_balance_key(correlation_id, uref.into())
        {
            Ok(key) => key,
            Err(_) => return false,
        };
        tracking_copy
            .borrow_mut()
            .get_purse_balance(correlation_id, key)
            .is_ok()
    }

    /// Resolves a source purse of the transfer.
    ///
    /// User can optionally pass a "source" argument which should refer to an [`URef`] existing in
    /// user's named keys. When the "source" argument is missing then user's main purse is assumed.
    ///
    /// Returns resolved [`URef`].
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
        let arg_name = mint::ARG_SOURCE;
        match imputed_runtime_args.get(arg_name) {
            Some(cl_value) if *cl_value.cl_type() == CLType::URef => {
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

    /// Resolves a transfer target mode.
    ///
    /// User has to specify a "target" argument which could be either an existing purse [`URef`] or
    /// an account hash. If the "target" account hash is not existing, then a special variant is
    /// returned that indicates that the system has to create new account first.
    ///
    /// Returns [`TransferTargetMode`] with a resolved variant.
    fn resolve_transfer_target_mode<R>(
        &mut self,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<TransferTargetMode, Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        let imputed_runtime_args = &self.inner;
        let arg_name = mint::ARG_TARGET;
        match imputed_runtime_args.get(arg_name) {
            Some(cl_value) if *cl_value.cl_type() == CLType::URef => {
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
            Some(cl_value) if *cl_value.cl_type() == CLType::ByteArray(32) => {
                let account_key: Key = {
                    let hash: AccountHash = match cl_value.clone().into_t() {
                        Ok(hash) => hash,
                        Err(error) => {
                            return Err(Error::Exec(ExecError::Revert(error.into())));
                        }
                    };
                    self.to = Some(hash.to_owned());
                    Key::Account(hash)
                };
                match account_key.into_account() {
                    Some(public_key) => {
                        match tracking_copy
                            .borrow_mut()
                            .read_account(correlation_id, public_key)
                        {
                            Ok(account) => Ok(TransferTargetMode::PurseExists(
                                account.main_purse().with_access_rights(AccessRights::ADD),
                            )),
                            Err(_) => Ok(TransferTargetMode::CreateAccount(public_key)),
                        }
                    }
                    None => Err(Error::Exec(ExecError::Revert(ApiError::Transfer))),
                }
            }
            Some(cl_value) if *cl_value.cl_type() == CLType::Key => {
                let account_key: Key = match cl_value.clone().into_t() {
                    Ok(key) => key,
                    Err(error) => {
                        return Err(Error::Exec(ExecError::Revert(error.into())));
                    }
                };
                match account_key.into_account() {
                    Some(account_hash) => {
                        self.to = Some(account_hash.to_owned());
                        match tracking_copy
                            .borrow_mut()
                            .read_account(correlation_id, account_hash)
                        {
                            Ok(account) => Ok(TransferTargetMode::PurseExists(
                                account.main_purse().with_access_rights(AccessRights::ADD),
                            )),
                            Err(_) => Ok(TransferTargetMode::CreateAccount(account_hash)),
                        }
                    }
                    None => Err(Error::Exec(ExecError::Revert(ApiError::Transfer))),
                }
            }
            Some(_) => Err(Error::Exec(ExecError::Revert(ApiError::InvalidArgument))),
            None => Err(Error::Exec(ExecError::Revert(ApiError::MissingArgument))),
        }
    }

    /// Resolves amount.
    ///
    /// User has to specify "amount" argument that could be either a [`U512`] or a u64.
    fn resolve_amount(&self) -> Result<U512, Error> {
        let imputed_runtime_args = &self.inner;
        match imputed_runtime_args.get(mint::ARG_AMOUNT) {
            Some(amount_value) if *amount_value.cl_type() == CLType::U512 => {
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
            Some(amount_value) if *amount_value.cl_type() == CLType::U64 => {
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

    /// Returns a resolved [`TransferTargetMode`].
    pub(crate) fn transfer_target_mode<R>(
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

    /// Creates new [`TransferArgs`] instance.
    pub fn build<R>(
        mut self,
        from: &Account,
        correlation_id: CorrelationId,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<TransferArgs, Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        let to = self.to;

        let target_uref =
            match self.resolve_transfer_target_mode(correlation_id, Rc::clone(&tracking_copy))? {
                TransferTargetMode::PurseExists(uref) => uref,
                _ => {
                    return Err(Error::Exec(ExecError::Revert(ApiError::Transfer)));
                }
            };

        let source_uref =
            self.resolve_source_uref(from, correlation_id, Rc::clone(&tracking_copy))?;

        if source_uref.addr() == target_uref.addr() {
            return Err(ExecError::Revert(ApiError::InvalidPurse).into());
        }

        let amount = self.resolve_amount()?;

        let id = {
            let id_bytes: Result<Option<u64>, _> = match self.inner.get(mint::ARG_ID) {
                Some(id_bytes) => id_bytes.clone().into_t(),
                None => return Err(ExecError::Revert(ApiError::MissingArgument).into()),
            };
            match id_bytes {
                Ok(id) => id,
                Err(err) => return Err(Error::Exec(ExecError::Revert(err.into()))),
            }
        };

        Ok(TransferArgs {
            to,
            source: source_uref,
            target: target_uref,
            amount,
            arg_id: id,
        })
    }
}
