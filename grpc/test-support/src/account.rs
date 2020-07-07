use std::convert::TryFrom;

use node::contract_shared;
use node::contract_shared::stored_value::StoredValue;
use types::{account::AccountHash, contracts::NamedKeys, URef};

use crate::{Error, Result};

/// An `Account` instance.
#[derive(Eq, PartialEq, Clone, Debug)]
pub struct Account {
    inner: contract_shared::account::Account,
}

impl Account {
    /// creates a new Account instance.
    pub(crate) fn new(account: contract_shared::account::Account) -> Self {
        Account { inner: account }
    }

    /// Returns the public_key.
    pub fn account_hash(&self) -> AccountHash {
        self.inner.account_hash()
    }

    /// Returns the named_keys.
    pub fn named_keys(&self) -> &NamedKeys {
        self.inner.named_keys()
    }

    /// Returns the main_purse.
    pub fn main_purse(&self) -> URef {
        self.inner.main_purse()
    }
}

impl From<contract_shared::account::Account> for Account {
    fn from(value: contract_shared::account::Account) -> Self {
        Account::new(value)
    }
}

impl TryFrom<StoredValue> for Account {
    type Error = Error;

    fn try_from(value: StoredValue) -> Result<Self> {
        match value {
            StoredValue::Account(account) => Ok(Account::new(account)),
            _ => Err(Error::from(String::from("StoredValue is not an Account"))),
        }
    }
}
