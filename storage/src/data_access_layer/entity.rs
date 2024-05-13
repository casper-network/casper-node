use serde::Serialize;
use thiserror::Error;
use tracing::error;

use casper_types::{
    account::AccountHash, addressable_entity::Weight, bytesrepr::FromBytes, system::entity,
    CLTyped, CLValue, CLValueError, RuntimeArgs, TransactionEntryPoint,
};

/// An error returned when constructing an [`EntityMethod`].
#[derive(Clone, Eq, PartialEq, Error, Serialize, Debug)]
pub enum EntityMethodError {
    /// Provided entry point is not one of the Entity ones.
    #[error("invalid entry point for entity: {0}")]
    InvalidEntryPoint(TransactionEntryPoint),
    /// Required arg missing.
    #[error("missing '{0}' arg")]
    MissingArg(String),
    /// Failed to parse the given arg.
    #[error("failed to parse '{arg}' arg: {error}")]
    CLValue {
        /// The arg name.
        arg: String,
        /// The failure.
        error: CLValueError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityMethod {
    AddAssociatedKey {
        account: AccountHash,
        weight: Weight,
    },
    UpdateAssociatedKey {
        account: AccountHash,
        weight: Weight,
    },
    RemoveAssociatedKey {
        account: AccountHash,
    },
}

impl EntityMethod {
    pub fn from_parts(
        entry_point: TransactionEntryPoint,
        runtime_args: &RuntimeArgs,
    ) -> Result<Self, EntityMethodError> {
        match entry_point {
            TransactionEntryPoint::AddAssociatedKey => Self::new_add_associated_key(runtime_args),
            TransactionEntryPoint::UpdateAssociatedKey => {
                Self::new_update_associated_key(runtime_args)
            }
            TransactionEntryPoint::RemoveAssociatedKey => {
                Self::new_remove_associated_key(runtime_args)
            }
            _ => Err(EntityMethodError::InvalidEntryPoint(entry_point)),
        }
    }

    fn new_add_associated_key(runtime_args: &RuntimeArgs) -> Result<Self, EntityMethodError> {
        let account = Self::get_named_argument(runtime_args, entity::ARG_ACCOUNT)?;
        let weight = Self::get_named_argument(runtime_args, entity::ARG_WEIGHT)?;
        Ok(Self::AddAssociatedKey { account, weight })
    }

    fn new_update_associated_key(runtime_args: &RuntimeArgs) -> Result<Self, EntityMethodError> {
        let account = Self::get_named_argument(runtime_args, entity::ARG_ACCOUNT)?;
        let weight = Self::get_named_argument(runtime_args, entity::ARG_WEIGHT)?;
        Ok(Self::UpdateAssociatedKey { account, weight })
    }

    fn new_remove_associated_key(runtime_args: &RuntimeArgs) -> Result<Self, EntityMethodError> {
        let account = Self::get_named_argument(runtime_args, entity::ARG_ACCOUNT)?;
        Ok(Self::RemoveAssociatedKey { account })
    }

    fn get_named_argument<T: FromBytes + CLTyped>(
        args: &RuntimeArgs,
        name: &str,
    ) -> Result<T, EntityMethodError> {
        let arg: &CLValue = args
            .get(name)
            .ok_or_else(|| EntityMethodError::MissingArg(name.to_string()))?;
        arg.to_t().map_err(|error| EntityMethodError::CLValue {
            arg: name.to_string(),
            error,
        })
    }
}
