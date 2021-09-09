//! Implementation of an `Address` which refers either an account hash, or a contract hash.
use alloc::vec::Vec;
use casper_types::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, ContractHash, Key,
};

/// An enum representing either an account hash, or a contract hash.
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
pub enum Address {
    /// Represents an account hash.
    Account(AccountHash),
    /// Represents a contract hash.
    Contract(ContractHash),
}

impl From<ContractHash> for Address {
    fn from(v: ContractHash) -> Self {
        Self::Contract(v)
    }
}

impl From<AccountHash> for Address {
    fn from(v: AccountHash) -> Self {
        Self::Account(v)
    }
}

impl Address {
    /// Creates new Key instance by consuming self. Returns either a `Key::Account` or a
    /// `Key::Contract`.
    pub fn into_key(self) -> Key {
        match self {
            Address::Account(account_hash) => Key::Account(account_hash),
            Address::Contract(contract_hash) => Key::Hash(contract_hash.value()),
        }
    }

    /// Returns wrapped account if this is an [`AccountHash`].
    pub fn as_account(&self) -> Option<&AccountHash> {
        if let Self::Account(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns wrapped contract hash if this is a [`ContractHash`].
    pub fn as_contract(&self) -> Option<&ContractHash> {
        if let Self::Contract(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl CLTyped for Address {
    fn cl_type() -> casper_types::CLType {
        CLType::Key
    }
}

impl ToBytes for Address {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.into_key().to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.into_key().serialized_length()
    }
}

impl FromBytes for Address {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (key, rem) = Key::from_bytes(bytes)?;

        let address = match key {
            Key::Account(account_hash) => Address::Account(account_hash),
            Key::Hash(raw_contract_hash) => {
                let contract_hash = ContractHash::new(raw_contract_hash);
                Address::Contract(contract_hash)
            }
            _ => return Err(bytesrepr::Error::Formatting),
        };

        Ok((address, rem))
    }
}
