#![no_std]

use core::str::FromStr;

use casper_types::ApiError;

pub const ARG_CONTRACT_HASH: &str = "payment_contract";
pub const ARG_CONTRACT_PACKAGE_HASH: &str = "contract_package_hash";
pub const ARG_TEST_METHOD: &str = "test_method";

#[repr(u16)]
pub enum Error {
    InvalidMethod = 0,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError::User(error as u16)
    }
}

pub const METHOD_CALL_DO_NOTHING: &str = "call_do_nothing";
pub const METHOD_CALL_VERSIONED_DO_NOTHING: &str = "call_versioned_do_nothing";
pub const METHOD_CALL_DO_NOTHING_NO_ARGS: &str = "call_do_nothing_no_args";
pub const METHOD_CALL_VERSIONED_DO_NOTHING_NO_ARGS: &str = "call_versioned_do_nothing_no_args";
pub const METHOD_CALL_DO_NOTHING_TYPE_MISMATCH: &str = "call_do_nothing_type_mismatch";
pub const METHOD_CALL_VERSIONED_DO_NOTHING_TYPE_MISMATCH: &str =
    "call_versioned_do_nothing_type_mismatch";
pub const METHOD_CALL_DO_NOTHING_NO_OPTIONALS: &str = "call_do_nothing_no_optionals";
pub const METHOD_CALL_VERSIONED_DO_NOTHING_NO_OPTIONALS: &str =
    "call_versioned_do_nothing_no_optionals";
pub const METHOD_CALL_DO_NOTHING_EXTRA: &str = "call_do_nothing_extra";
pub const METHOD_CALL_VERSIONED_DO_NOTHING_EXTRA: &str = "call_versioned_do_nothing_extra";

pub enum TestMethod {
    CallDoNothing,
    CallVersionedDoNothing,
    CallDoNothingNoArgs,
    CallVersionedDoNothingNoArgs,
    CallDoNothingTypeMismatch,
    CallVersionedDoNothingTypeMismatch,
    CallDoNothingNoOptionals,
    CallVersionedDoNothingNoOptionals,
    CallDoNothingExtra,
    CallVersionedDoNothingExtra,
}

impl FromStr for TestMethod {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == METHOD_CALL_DO_NOTHING {
            Ok(TestMethod::CallDoNothing)
        } else if s == METHOD_CALL_VERSIONED_DO_NOTHING {
            Ok(TestMethod::CallVersionedDoNothing)
        } else if s == METHOD_CALL_DO_NOTHING_NO_ARGS {
            Ok(TestMethod::CallDoNothingNoArgs)
        } else if s == METHOD_CALL_VERSIONED_DO_NOTHING_NO_ARGS {
            Ok(TestMethod::CallVersionedDoNothingNoArgs)
        } else if s == METHOD_CALL_DO_NOTHING_TYPE_MISMATCH {
            Ok(TestMethod::CallDoNothingTypeMismatch)
        } else if s == METHOD_CALL_VERSIONED_DO_NOTHING_TYPE_MISMATCH {
            Ok(TestMethod::CallVersionedDoNothingTypeMismatch)
        } else if s == METHOD_CALL_DO_NOTHING_NO_OPTIONALS {
            Ok(TestMethod::CallDoNothingNoOptionals)
        } else if s == METHOD_CALL_VERSIONED_DO_NOTHING_NO_OPTIONALS {
            Ok(TestMethod::CallVersionedDoNothingNoOptionals)
        } else if s == METHOD_CALL_DO_NOTHING_EXTRA {
            Ok(TestMethod::CallDoNothingExtra)
        } else if s == METHOD_CALL_VERSIONED_DO_NOTHING_EXTRA {
            Ok(TestMethod::CallVersionedDoNothingExtra)
        } else {
            Err(Error::InvalidMethod)
        }
    }
}
