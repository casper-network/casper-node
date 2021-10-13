use std::{cell::RefCell, convert::TryFrom, rc::Rc};

use casper_types::{
    account::{Account, AccountHash},
    system::mint,
    AccessRights, ApiError, CLType, CLValueError, Key, PublicKey, RuntimeArgs, StoredValue, URef,
    U512,
};

use crate::{
    core::{
        engine_state::Error,
        execution::Error as ExecError,
        tracking_copy::{TrackingCopy, TrackingCopyExt},
    },
    shared::newtypes::CorrelationId,
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

/// State of a builder of a `TransferArgs`.
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

    /// Resolves the source purse of the transfer.
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
                let uref: URef = cl_value.clone().into_t().map_err(Error::reverter)?;

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
                                return Err(Error::reverter(ApiError::InvalidPurse));
                            }

                            Ok(uref)
                        } else {
                            Err(Error::Exec(ExecError::InvalidAccess {
                                required: AccessRights::WRITE,
                            }))
                        }
                    }
                    Some(key) => Err(Error::Exec(ExecError::TypeMismatch(
                        casper_types::StoredValueTypeMismatch::new(
                            "Key::URef".to_string(),
                            key.type_string(),
                        ),
                    ))),
                    None => Err(Error::Exec(ExecError::ForgedReference(uref))),
                }
            }
            Some(_) => Err(Error::reverter(ApiError::InvalidArgument)),
            None => Ok(account.main_purse()), // if no source purse passed use account main purse
        }
    }

    /// Resolves a transfer target mode.
    ///
    /// User has to specify a "target" argument which must be one of the following types:
    ///   * an existing purse [`URef`]
    ///   * a 32-byte array, interpreted as an account hash
    ///   * a [`Key::Account`], from which the account hash is extracted
    ///   * a [`PublicKey`], which is converted to an account hash
    ///
    /// If the "target" account hash is not existing, then a special variant is returned that
    /// indicates that the system has to create new account first.
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

        let account_hash = match imputed_runtime_args.get(arg_name) {
            Some(cl_value) if *cl_value.cl_type() == CLType::URef => {
                let uref: URef = cl_value.clone().into_t().map_err(Error::reverter)?;

                if !self.purse_exists(uref, correlation_id, tracking_copy) {
                    return Err(Error::reverter(ApiError::InvalidPurse));
                }

                return Ok(TransferTargetMode::PurseExists(uref));
            }
            Some(cl_value) if *cl_value.cl_type() == CLType::ByteArray(32) => {
                let account_hash: AccountHash =
                    cl_value.clone().into_t().map_err(Error::reverter)?;
                account_hash
            }
            Some(cl_value) if *cl_value.cl_type() == CLType::Key => {
                let account_key: Key = cl_value.clone().into_t().map_err(Error::reverter)?;

                let account_hash: AccountHash = account_key
                    .into_account()
                    .ok_or_else(|| Error::reverter(ApiError::Transfer))?;
                account_hash
            }
            Some(cl_value) if *cl_value.cl_type() == CLType::PublicKey => {
                let public_key: PublicKey = cl_value.clone().into_t().map_err(Error::reverter)?;

                AccountHash::from(&public_key)
            }
            Some(_) => return Err(Error::reverter(ApiError::InvalidArgument)),
            None => return Err(Error::reverter(ApiError::MissingArgument)),
        };

        self.to = Some(account_hash);
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

    /// Resolves amount.
    ///
    /// User has to specify "amount" argument that could be either a [`U512`] or a u64.
    fn resolve_amount(&self) -> Result<U512, Error> {
        let imputed_runtime_args = &self.inner;

        let amount = match imputed_runtime_args.get(mint::ARG_AMOUNT) {
            Some(amount_value) if *amount_value.cl_type() == CLType::U512 => amount_value
                .clone()
                .into_t::<U512>()
                .map_err(Error::reverter)?,
            Some(amount_value) if *amount_value.cl_type() == CLType::U64 => {
                let amount = amount_value
                    .clone()
                    .into_t::<u64>()
                    .map_err(Error::reverter)?;
                U512::from(amount)
            }
            Some(_) => return Err(Error::reverter(ApiError::InvalidArgument)),
            None => return Err(Error::reverter(ApiError::MissingArgument)),
        };

        if amount.is_zero() {
            return Err(Error::reverter(ApiError::Transfer));
        }

        Ok(amount)
    }

    fn resolve_id(&self) -> Result<Option<u64>, Error> {
        let id_value = self
            .inner
            .get(mint::ARG_ID)
            .ok_or_else(|| Error::reverter(ApiError::MissingArgument))?;
        let id: Option<u64> = id_value.clone().into_t().map_err(Error::reverter)?;
        Ok(id)
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
                    return Err(Error::reverter(ApiError::Transfer));
                }
            };

        let source_uref =
            self.resolve_source_uref(from, correlation_id, Rc::clone(&tracking_copy))?;

        if source_uref.addr() == target_uref.addr() {
            return Err(Error::reverter(ApiError::InvalidPurse));
        }

        let amount = self.resolve_amount()?;

        let id = self.resolve_id()?;

        Ok(TransferArgs {
            to,
            source: source_uref,
            target: target_uref,
            amount,
            arg_id: id,
        })
    }
}
