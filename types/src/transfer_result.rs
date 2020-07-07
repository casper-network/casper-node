use core::fmt::Debug;

use crate::ApiError;

/// The result of an attempt to transfer between purses.
pub type TransferResult = Result<TransferredTo, ApiError>;

/// The result of a successful transfer between purses.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(i32)]
pub enum TransferredTo {
    /// The destination account already existed.
    ExistingAccount = 0,
    /// The destination account was created.
    NewAccount = 1,
}

impl TransferredTo {
    /// Converts an `i32` to a [`TransferResult`], where:
    /// * `0` represents `Ok(TransferredTo::ExistingAccount)`,
    /// * `1` represents `Ok(TransferredTo::NewAccount)`,
    /// * all other inputs are mapped to `Err(ApiError::Transfer)`.
    pub fn result_from(value: i32) -> TransferResult {
        match value {
            x if x == TransferredTo::ExistingAccount as i32 => Ok(TransferredTo::ExistingAccount),
            x if x == TransferredTo::NewAccount as i32 => Ok(TransferredTo::NewAccount),
            _ => Err(ApiError::Transfer),
        }
    }

    // This conversion is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn i32_from(result: TransferResult) -> i32 {
        match result {
            Ok(transferred_to) => transferred_to as i32,
            Err(_) => 2,
        }
    }
}
