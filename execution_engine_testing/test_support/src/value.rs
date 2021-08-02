use std::convert::{TryFrom, TryInto};

use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    stored_value::StoredValue,
    CLTyped, CLValue,
};

use crate::{Account, Result};

/// A value stored under a given key on the network.
#[derive(Eq, PartialEq, Clone, Debug)]
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
    pub fn from_t<T: CLTyped + ToBytes>(t: T) -> Result<Value> {
        let cl_value = CLValue::from_t(t)?;
        let inner = StoredValue::CLValue(cl_value);
        Ok(Value { inner })
    }

    /// Consumes and converts `self` back into its underlying type.
    pub fn into_t<T: CLTyped + FromBytes>(self) -> Result<T> {
        let cl_value = CLValue::try_from(self.inner)?;
        Ok(cl_value.into_t()?)
    }

    /// Consumes and converts `self` into an `Account` or errors.
    pub fn into_account(self) -> Result<Account> {
        self.inner.try_into()
    }
}
