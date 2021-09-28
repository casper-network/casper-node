use std::convert::{TryFrom, TryInto};

use thiserror::Error;

use casper_types::{
    account::Account,
    bytesrepr::{FromBytes, ToBytes},
    CLTypeMismatch, CLTyped, CLValue, CLValueError, StoredValue, StoredValueTypeMismatch,
};

#[derive(Error, Debug, PartialEq)]
pub enum Error {
    #[error("Type Error: {0}.")]
    TypeError(StoredValueTypeMismatch),
    #[error("Value Error: {0}.")]
    ValueError(CLValueError),
}

impl From<StoredValueTypeMismatch> for Error {
    fn from(e: StoredValueTypeMismatch) -> Error {
        Error::TypeError(e)
    }
}

impl From<CLValueError> for Error {
    fn from(e: CLValueError) -> Error {
        Error::ValueError(e)
    }
}

/// A value stored under a given key on the network.
#[derive(Eq, PartialEq, Clone, Debug)]
#[deprecated]
pub struct Value {
    inner: StoredValue,
}

impl Value {
    pub(crate) fn new(stored_value: StoredValue) -> Self {
        Value {
            inner: stored_value,
        }
    }

    /// Constructs a `Value` from `t`.
    pub fn from_t<T: CLTyped + ToBytes>(t: T) -> Result<Value, CLValueError> {
        let cl_value = CLValue::from_t(t)?;
        let inner = StoredValue::CLValue(cl_value);
        Ok(Value { inner })
    }

    /// Consumes and converts `self` back into its underlying type.
    pub fn into_t<T: CLTyped + FromBytes>(self) -> Result<T, Error> {
        let cl_value = CLValue::try_from(self.inner)?;
        Ok(cl_value.into_t()?)
    }

    /// Consumes and converts `self` into an `Account` or errors.
    pub fn into_account(self) -> std::result::Result<Account, StoredValueTypeMismatch> {
        self.inner.try_into()
    }
}
