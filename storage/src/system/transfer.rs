use std::{cell::RefCell, convert::TryFrom, rc::Rc};
use thiserror::Error;

use casper_types::{
    account::AccountHash,
    bytesrepr::FromBytes,
    system::{mint, mint::Error as MintError},
    AccessRights, CLType, CLTyped, CLValue, CLValueError, Key, ProtocolVersion, RuntimeArgs,
    RuntimeFootprint, StoredValue, StoredValueTypeMismatch, URef, U512,
};

use crate::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
};

/// Transfer error.
#[derive(Clone, Error, Debug)]
pub enum TransferError {
    /// Invalid key variant.
    #[error("Invalid key {0}")]
    UnexpectedKeyVariant(Key),
    /// Type mismatch error.
    #[error("{}", _0)]
    TypeMismatch(StoredValueTypeMismatch),
    /// Forged reference error.
    #[error("Forged reference: {}", _0)]
    ForgedReference(URef),
    /// Invalid access.
    #[error("Invalid access rights: {}", required)]
    InvalidAccess {
        /// Required access rights of the operation.
        required: AccessRights,
    },
    /// Error converting a CLValue.
    #[error("{0}")]
    CLValue(CLValueError),
    /// Invalid purse.
    #[error("Invalid purse")]
    InvalidPurse,
    /// Invalid argument.
    #[error("Invalid argument")]
    InvalidArgument,
    /// Missing argument.
    #[error("Missing argument")]
    MissingArgument,
    /// Invalid purse.
    #[error("Attempt to transfer amount 0")]
    AttemptToTransferZero,
    /// Invalid operation.
    #[error("Invalid operation")]
    InvalidOperation,
    /// Disallowed transfer attempt (private chain).
    #[error("Either the source or the target must be an admin (private chain).")]
    RestrictedTransferAttempted,
    /// Could not determine if target is an admin (private chain).
    #[error("Unable to determine if the target of a transfer is an admin")]
    UnableToVerifyTargetIsAdmin,
    /// Tracking copy error.
    #[error("{0}")]
    TrackingCopy(TrackingCopyError),
    /// Mint error.
    #[error("{0}")]
    Mint(MintError),
}

impl From<GlobalStateError> for TransferError {
    fn from(gse: GlobalStateError) -> Self {
        TransferError::TrackingCopy(TrackingCopyError::Storage(gse))
    }
}

impl From<TrackingCopyError> for TransferError {
    fn from(tce: TrackingCopyError) -> Self {
        TransferError::TrackingCopy(tce)
    }
}

/// A target mode indicates if a native transfer's arguments will resolve to an existing purse, or
/// will have to create a new account first.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TransferTargetMode {
    /// Native transfer arguments resolved into a transfer to an existing account.
    ExistingAccount {
        /// Existing account hash.
        target_account_hash: AccountHash,
        /// Main purse of a resolved account.
        main_purse: URef,
    },
    /// Native transfer arguments resolved into a transfer to a purse.
    PurseExists {
        /// Target account hash (if known).
        target_account_hash: Option<AccountHash>,
        /// Purse.
        purse_uref: URef,
    },
    /// Native transfer arguments resolved into a transfer to a new account.
    CreateAccount(AccountHash),
}

impl TransferTargetMode {
    /// Target account hash, if any.
    pub fn target_account_hash(&self) -> Option<AccountHash> {
        match self {
            TransferTargetMode::PurseExists {
                target_account_hash,
                ..
            } => *target_account_hash,
            TransferTargetMode::ExistingAccount {
                target_account_hash,
                ..
            } => Some(*target_account_hash),
            TransferTargetMode::CreateAccount(target_account_hash) => Some(*target_account_hash),
        }
    }
}

/// Mint's transfer arguments.
///
/// A struct has a benefit of static typing, which is helpful while resolving the arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Returns `target` field.
    pub fn target(&self) -> URef {
        self.target
    }

    /// Returns `amount` field.
    pub fn amount(&self) -> U512 {
        self.amount
    }

    /// Returns `arg_id` field.
    pub fn arg_id(&self) -> Option<u64> {
        self.arg_id
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
/// Purpose of this builder is to resolve native transfer args into [`TransferTargetMode`] and a
/// [`TransferArgs`] instance to execute actual token transfer on the mint contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferRuntimeArgsBuilder {
    inner: RuntimeArgs,
}

impl TransferRuntimeArgsBuilder {
    /// Creates new transfer args builder.
    ///
    /// Takes an incoming runtime args that represents native transfer's arguments.
    pub fn new(imputed_runtime_args: RuntimeArgs) -> TransferRuntimeArgsBuilder {
        TransferRuntimeArgsBuilder {
            inner: imputed_runtime_args,
        }
    }

    /// Checks if a purse exists.
    fn purse_exists<R>(&self, uref: URef, tracking_copy: Rc<RefCell<TrackingCopy<R>>>) -> bool
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let key = match tracking_copy
            .borrow_mut()
            .get_purse_balance_key(uref.into())
        {
            Ok(key) => key,
            Err(_) => return false,
        };
        tracking_copy
            .borrow_mut()
            .get_available_balance(key)
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
        account: &RuntimeFootprint,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<URef, TransferError>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let imputed_runtime_args = &self.inner;
        let arg_name = mint::ARG_SOURCE;
        let uref = match imputed_runtime_args.get(arg_name) {
            Some(cl_value) if *cl_value.cl_type() == CLType::URef => {
                self.map_cl_value::<URef>(cl_value)?
            }
            Some(cl_value) if *cl_value.cl_type() == CLType::Option(CLType::URef.into()) => {
                let Some(uref): Option<URef> = self.map_cl_value(cl_value)? else {
                    return account.main_purse().ok_or(TransferError::InvalidOperation);
                };
                uref
            }
            Some(_) => return Err(TransferError::InvalidArgument),
            None => return account.main_purse().ok_or(TransferError::InvalidOperation), /* if no source purse passed use account
                                                                                         * main purse */
        };
        if account
            .main_purse()
            .ok_or(TransferError::InvalidOperation)?
            .addr()
            == uref.addr()
        {
            return Ok(uref);
        }

        let normalized_uref = Key::URef(uref).normalize();
        let maybe_named_key = account
            .named_keys()
            .keys()
            .find(|&named_key| named_key.normalize() == normalized_uref);

        match maybe_named_key {
            Some(Key::URef(found_uref)) => {
                if found_uref.is_writeable() {
                    // it is a URef and caller has access but is it a purse URef?
                    if !self.purse_exists(found_uref.to_owned(), tracking_copy) {
                        return Err(TransferError::InvalidPurse);
                    }

                    Ok(uref)
                } else {
                    Err(TransferError::InvalidAccess {
                        required: AccessRights::WRITE,
                    })
                }
            }
            Some(key) => Err(TransferError::TypeMismatch(StoredValueTypeMismatch::new(
                "Key::URef".to_string(),
                key.type_string(),
            ))),
            None => Err(TransferError::ForgedReference(uref)),
        }
    }

    /// Resolves a transfer target mode.
    ///
    /// User has to specify a "target" argument which must be one of the following types:
    ///   * an existing purse [`URef`]
    ///   * a 32-byte array, interpreted as an account hash
    ///   * a [`Key::Account`], from which the account hash is extracted
    ///   * a [`casper_types::PublicKey`], which is converted to an account hash
    ///
    /// If the "target" account hash is not existing, then a special variant is returned that
    /// indicates that the system has to create new account first.
    ///
    /// Returns [`TransferTargetMode`] with a resolved variant.
    pub fn resolve_transfer_target_mode<R>(
        &mut self,
        protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<TransferTargetMode, TransferError>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let imputed_runtime_args = &self.inner;
        let to_name = mint::ARG_TO;

        let target_account_hash = match imputed_runtime_args.get(to_name) {
            Some(cl_value)
                if *cl_value.cl_type() == CLType::Option(Box::new(CLType::ByteArray(32))) =>
            {
                let to: Option<AccountHash> = self.map_cl_value(cl_value)?;
                to
            }
            Some(_) | None => None,
        };

        let target_name = mint::ARG_TARGET;
        let account_hash = match imputed_runtime_args.get(target_name) {
            Some(cl_value) if *cl_value.cl_type() == CLType::URef => {
                let purse_uref = self.map_cl_value(cl_value)?;

                if !self.purse_exists(purse_uref, tracking_copy) {
                    return Err(TransferError::InvalidPurse);
                }

                return Ok(TransferTargetMode::PurseExists {
                    purse_uref,
                    target_account_hash,
                });
            }
            Some(cl_value) if *cl_value.cl_type() == CLType::ByteArray(32) => {
                self.map_cl_value(cl_value)?
            }
            Some(cl_value) if *cl_value.cl_type() == CLType::Key => {
                let account_key: Key = self.map_cl_value(cl_value)?;
                let account_hash: AccountHash = account_key
                    .into_account()
                    .ok_or(TransferError::UnexpectedKeyVariant(account_key))?;
                account_hash
            }
            Some(cl_value) if *cl_value.cl_type() == CLType::PublicKey => {
                let public_key = self.map_cl_value(cl_value)?;
                AccountHash::from(&public_key)
            }
            Some(_) => return Err(TransferError::InvalidArgument),
            None => return Err(TransferError::MissingArgument),
        };

        match tracking_copy
            .borrow_mut()
            .runtime_footprint_by_account_hash(protocol_version, account_hash)
        {
            Ok((_, entity)) => {
                let main_purse_addable = entity
                    .main_purse()
                    .ok_or(TransferError::InvalidPurse)?
                    .with_access_rights(AccessRights::ADD);
                Ok(TransferTargetMode::ExistingAccount {
                    target_account_hash: account_hash,
                    main_purse: main_purse_addable,
                })
            }
            Err(_) => Ok(TransferTargetMode::CreateAccount(account_hash)),
        }
    }

    /// Resolves amount.
    ///
    /// User has to specify "amount" argument that could be either a [`U512`] or a u64.
    fn resolve_amount(&self) -> Result<U512, TransferError> {
        let imputed_runtime_args = &self.inner;

        let amount = match imputed_runtime_args.get(mint::ARG_AMOUNT) {
            Some(amount_value) if *amount_value.cl_type() == CLType::U512 => {
                self.map_cl_value(amount_value)?
            }
            Some(amount_value) if *amount_value.cl_type() == CLType::U64 => {
                let amount: u64 = self.map_cl_value(amount_value)?;
                U512::from(amount)
            }
            Some(_) => return Err(TransferError::InvalidArgument),
            None => return Err(TransferError::MissingArgument),
        };

        if amount.is_zero() {
            return Err(TransferError::AttemptToTransferZero);
        }

        Ok(amount)
    }

    fn resolve_id(&self) -> Result<Option<u64>, TransferError> {
        let id: Option<u64> = if let Some(id_value) = self.inner.get(mint::ARG_ID) {
            self.map_cl_value(id_value)?
        } else {
            None
        };
        Ok(id)
    }

    /// Creates new [`TransferArgs`] instance.
    pub fn build<R>(
        mut self,
        from: &RuntimeFootprint,
        protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    ) -> Result<TransferArgs, TransferError>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let (to, target) = match self
            .resolve_transfer_target_mode(protocol_version, Rc::clone(&tracking_copy))?
        {
            TransferTargetMode::ExistingAccount {
                main_purse: purse_uref,
                target_account_hash: target_account,
            } => (Some(target_account), purse_uref),
            TransferTargetMode::PurseExists {
                target_account_hash,
                purse_uref,
            } => (target_account_hash, purse_uref),
            TransferTargetMode::CreateAccount(_) => {
                // Method "build()" is called after `resolve_transfer_target_mode` is first called
                // and handled by creating a new account. Calling `resolve_transfer_target_mode`
                // for the second time should never return `CreateAccount` variant.
                return Err(TransferError::InvalidOperation);
            }
        };

        let source = self.resolve_source_uref(from, Rc::clone(&tracking_copy))?;

        if source.addr() == target.addr() {
            return Err(TransferError::InvalidPurse);
        }

        let amount = self.resolve_amount()?;

        let arg_id = self.resolve_id()?;

        Ok(TransferArgs {
            to,
            source,
            target,
            amount,
            arg_id,
        })
    }

    fn map_cl_value<T: CLTyped + FromBytes>(&self, cl_value: &CLValue) -> Result<T, TransferError> {
        cl_value.clone().into_t().map_err(TransferError::CLValue)
    }
}
