#![no_std]

extern crate alloc;

use alloc::str::FromStr;

use casper_types::ApiError;

pub const ARG_OPERATION: &str = "operation";
pub const ARG_CONTRACT_HASH: &str = "contract_hash";
pub const OP_WRITE: &str = "write";
pub const OP_READ: &str = "read";
pub const OP_FORGED_UREF_WRITE: &str = "forged_uref_write";
pub const OP_INVALID_PUT_DICTIONARY_ITEM_KEY: &str = "invalid_put_dictionary_item_key";
pub const OP_INVALID_GET_DICTIONARY_ITEM_KEY: &str = "invalid_get_dictionary_item_key";
pub const NEW_DICTIONARY_ITEM_KEY: &str = "New key";
pub const NEW_DICTIONARY_VALUE: &str = "New value";
pub const ARG_SHARE_UREF_ENTRYPOINT: &str = "share_uref_entrypoint";
pub const ARG_FORGED_UREF: &str = "forged_uref";

#[repr(u16)]
pub enum Error {
    InvalidOperation,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError::User(error as u16)
    }
}

pub enum Operation {
    Write,
    Read,
    ForgedURefWrite,
    InvalidPutDictionaryItemKey,
    InvalidGetDictionaryItemKey,
}

impl FromStr for Operation {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == OP_WRITE {
            Ok(Operation::Write)
        } else if s == OP_READ {
            Ok(Operation::Read)
        } else if s == OP_FORGED_UREF_WRITE {
            Ok(Operation::ForgedURefWrite)
        } else if s == OP_INVALID_PUT_DICTIONARY_ITEM_KEY {
            Ok(Operation::InvalidPutDictionaryItemKey)
        } else if s == OP_INVALID_GET_DICTIONARY_ITEM_KEY {
            Ok(Operation::InvalidGetDictionaryItemKey)
        } else {
            Err(Error::InvalidOperation)
        }
    }
}
