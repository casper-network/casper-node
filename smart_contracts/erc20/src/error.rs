//! Error handling on the casper platform.
use casper_types::ApiError;

/// Represents error conditions of the erc20 contract.
#[repr(u16)]
pub enum Error {
    /// ERC20 contract called from within invalid context.
    InvalidContext = 0,
    /// Spender does not have enough balance.
    InsufficientBalance = 1,
    /// Spender does not have enough allowance approved.
    InsufficientAllowance = 2,
    /// Operation would cause an integer overflow.
    Overflow = 3,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError::User(error as u16)
    }
}
